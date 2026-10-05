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
//! | `RenderErrorBox` | `harness_render_error_box_*` | yes | — | — | yes | — |
//! | `RenderCustomPaint` | `harness_custom_paint_*` | yes | yes | yes | yes | order, paint size, poison |
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
//! | `RenderContainer` | `harness_container_*` | yes | yes | yes | yes | yes |
//! | `RenderLayoutBuilder` | `harness_layout_builder_*` | yes | — | — | yes | dry |
//! | `RenderLimitedBox` | `harness_limited_box_*` | yes | — | — | yes | — |
//! | `RenderOffstage` | `harness_offstage_*` | yes | yes | — | yes | — |
//! | `RenderOpacity` | `harness_opacity_*` | yes | — | yes | yes | queries |
//! | `RenderVisibility` | `harness_visibility_*` | yes | — | yes | yes | queries |
//! | `RenderAnimatedOpacity` | `harness_animated_opacity_*` | yes | yes | yes | yes | tick dirty-marking |
//! | `RenderTransform` | `harness_transform_*` | yes | — | yes | yes | paint transform |
//! | `RenderFittedBox` | `harness_fitted_box_*` | yes | — | — | yes | paint transform |
//! | `RenderFractionallySizedBox` | `harness_fractionally_sized_box_*` | yes | — | — | yes | — |
//! | `RenderFractionalTranslation` | `harness_fractional_translation_*` | yes | — | — | yes | paint transform |
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
//! | `RenderSubtreeAnchor` | `harness_subtree_anchor_*` | yes | yes | yes | yes | attach/detach identity |
//! | `RenderSemanticsAnnotations` | `harness_semantics_annotations_*` | yes | — | — | yes | semantics |
//! | `RenderIndexedSemantics` | `harness_indexed_semantics_*` | yes | — | — | yes | semantics |
//! | `RenderMergeSemantics` | `harness_merge_semantics_*` | yes | — | — | yes | semantics |
//! | `RenderExcludeSemantics` | `harness_exclude_semantics_*` | yes | — | — | yes | semantics |
//! | `RenderMetaData` | `harness_metadata_*` | yes | — | — | yes | — |
//! | `RenderFlex` | `harness_flex_*` | yes | — | — | yes | queries, baseline |
//! | `RenderStack` | `harness_stack_*` | yes | yes | — | yes | queries |
//! | `RenderIndexedStack` | `harness_indexed_stack_*` | yes | yes | yes | yes | baseline |
//! | `RenderTheater` | `harness_theater_*` | yes | yes | yes | yes | skip_count |
//! | `RenderListBody` | `harness_list_body_*` | yes | yes | — | yes | dry baseline |
//! | `RenderFlow` | `harness_flow_*` | yes | yes | yes | yes | order, paint transform |
//! | `RenderTable` | `harness_table_*` | yes | yes | yes | yes | column widths |
//! | `RenderAbsorbPointer` | `harness_absorb_pointer_*` | yes | yes | — | yes | — |
//! | `RenderIgnorePointer` | `harness_ignore_pointer_*` | yes | yes | — | yes | — |
//! | `RenderIgnoreBaseline` | `harness_ignore_baseline_*` | yes | yes | — | yes | baseline hidden |
//! | `RenderListener` | `harness_listener_*` | yes | yes | — | yes | — |
//! | `RenderMouseRegion` | `harness_mouse_region_*` | yes | yes | — | yes | cursor/annotation |
//! | `RenderSliverFixedExtentList` | `harness_sliver_fixed_extent_list_*` | yes | — | — | yes | — |
//! | `RenderSliverGrid` | `harness_render_sliver_grid_*` | yes | yes | — | yes | — |
//! | `RenderSliverPadding` | `harness_sliver_padding_*` | yes | — | — | yes | — |
//! | `RenderSliverToBoxAdapter` | `harness_sliver_to_box_adapter_*` | yes | — | — | yes | — |
//! | `RenderSliverFillViewport` | `harness_sliver_fill_viewport_*` | yes | — | — | yes | — |
//! | `RenderSliverFillRemaining` | `harness_sliver_fill_remaining_*` | yes | — | — | yes | — |
//! | `RenderSliverFillRemainingAndOverscroll` | `harness_sliver_fill_remaining_and_overscroll_*` | yes | — | — | yes | — |
//! | `RenderSliverFillRemainingWithScrollable` | `harness_sliver_fill_remaining_with_scrollable_*` | yes | — | — | yes | — |
//! | `RenderSliverIgnorePointer` | `harness_sliver_ignore_pointer_*` | yes | yes | — | yes | — |
//! | `RenderSliverList` | `harness_sliver_list_*` | yes | — | — | yes | — |
//! | `RenderSliverMainAxisGroup` | `harness_sliver_main_axis_group_*` | yes | — | — | yes | — |
//! | `RenderSliverOffstage` | `harness_sliver_offstage_*` | yes | — | — | yes | — |
//! | `RenderSliverOpacity` | `harness_sliver_opacity_*` | yes | — | yes | yes | compositing |
//! | `RenderSliverAnimatedOpacity` | `harness_sliver_animated_opacity_*` | yes | yes | yes | yes | tick dirty-marking |
//! | `RenderViewport` | `harness_viewport_*` | yes | — | — | yes | — |
//! | `RenderShrinkWrappingViewport` | `harness_shrink_wrapping_viewport_*` | yes | — | — | yes | — |
//! | `RenderWrap` | `harness_render_wrap_*` | yes | yes | — | yes | — |
//! | `RenderIntrinsicWidth` | `harness_intrinsic_width_*` | yes | — | — | yes | queries |
//! | `RenderIntrinsicHeight` | `harness_intrinsic_height_*` | yes | — | — | yes | queries |
//! | `RenderConstrainedOverflowBox` | `harness_constrained_overflow_box_*` | yes | — | — | yes | — |
//! | `RenderSizedOverflowBox` | `harness_sized_overflow_box_*` | yes | — | — | yes | — |
//! | `RenderConstraintsTransformBox` | `harness_constraints_transform_box_*` | yes | — | yes | yes | intrinsics |
//! | `RenderRotatedBox` | `harness_rotated_box_*` | yes | yes | — | yes | paint transform |
//! | `RenderAnimatedSize` | `harness_render_animated_size_*` | yes | — | yes | yes | state machine |
//! | `RenderSliverScrollingPersistentHeader` | `harness_sliver_persistent_header_scrolling_*` | yes | — | — | — | — |
//! | `RenderSliverPinnedPersistentHeader` | `harness_sliver_persistent_header_pinned_*` | yes | — | — | — | viewport wiring |
//! | `RenderSliverFloatingPersistentHeader` | `harness_sliver_persistent_header_floating_*` | yes | — | — | — | state machine |
//! | `RenderSliverFloatingPinnedPersistentHeader` | `harness_sliver_persistent_header_floating_pinned_*` | yes | — | — | — | state machine |
//!
//! [`catalog_covers_every_render_object_name`] guards the table: every row's
//! type string must appear in this file so a missing harness test fails CI.

// Row functions are plain `fn`s run by the family tables below, so clippy no
// longer treats them as `#[test]` bodies where `unwrap` is allowed.
#![allow(clippy::unwrap_used)]

// Single-binary consolidation (`autotests = false` in `Cargo.toml`): the
// snapshot dogfood suite compiles as a module of this target instead of
// linking the full dependency stack a second time. Its insta snapshots are
// prefixed `render_object_harness__harness_snapshot__` accordingly.
#[path = "harness_snapshot.rs"]
mod harness_snapshot;

use std::{any::Any, cell::Cell, collections::HashMap, rc::Rc, sync::Arc, time::Duration};

use flui_animation::curve::ArcCurve;
use flui_animation::{Animation, AnimationController, Curves, ProxyAnimation, UpdateScheduler};
use flui_foundation::geometry::Axis;
use flui_foundation::geometry::{EdgeInsets, Matrix4, Offset, Point, Rect, Size};
use flui_interaction::InteractionLane;
use flui_interaction::routing::{MouseTracker, PointerMotionKind};
use flui_objects::TableColumnWidth;
use flui_objects::*;
use flui_painting::{Alignment, BoxFit, BoxShape};
use flui_painting::{Canvas, Paint};
use flui_painting::{
    paint::{Clip, ImageFilter, Path, Shader},
    styling::{BorderSide, BorderStyle, BoxDecoration, Color, TableBorder},
    typography::{TextDirection, TextSpan},
};
use flui_rendering::constraints::AxisDirection;
use flui_rendering::parent_data::TableCellVerticalAlignment;
use flui_rendering::{
    constraints::{BoxConstraints, SliverConstraints},
    context::BoxIntrinsicsCtx,
    delegates::{
        CustomPainter, FlowDelegate, FlowPaintingContext, MultiChildLayoutContext,
        MultiChildLayoutDelegate, SingleChildLayoutDelegate, SliverGridDelegate,
        SliverGridDelegateWithFixedCrossAxisCount, SliverGridLayout,
    },
    hit_testing::{HitTestBehavior, HitTestResult, MouseRegionCallbacks},
    layer::LayerLink,
    parent_data::{
        MultiChildLayoutParentData, SliverMultiBoxAdaptorParentData, StackParentData,
        TableCellParentData,
    },
    semantics::SemanticsProperties,
    testing::{
        BoxQueryRun, DrawKind, ParentDataSeed, Probe, RenderTester, TreeNode,
        assert_descendant_properties, assert_has_committed_geometry, assert_has_committed_size,
        box_node, sliver_node,
    },
    traits::{RenderBox, TextBaseline},
    view::{ScrollDirection, ScrollableViewportOffset},
};

/// Every concrete render-object type exported from `flui_objects`.
const RENDER_OBJECT_TYPES: &[&str] = &[
    "RenderAlign",
    "RenderSizedBox",
    "RenderColoredBox",
    "RenderErrorBox",
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
    "RenderContainer",
    "RenderLayoutBuilder",
    "RenderLimitedBox",
    "RenderOffstage",
    "RenderOpacity",
    "RenderVisibility",
    "RenderAnimatedOpacity",
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
    "RenderSubtreeAnchor",
    "RenderSemanticsAnnotations",
    "RenderIndexedSemantics",
    "RenderMergeSemantics",
    "RenderExcludeSemantics",
    "RenderMetaData",
    "RenderFlex",
    "RenderStack",
    "RenderIndexedStack",
    "RenderListBody",
    "RenderFlow",
    "RenderTable",
    "RenderTheater",
    "RenderAbsorbPointer",
    "RenderIgnorePointer",
    "RenderIgnoreBaseline",
    "RenderListener",
    "RenderMouseRegion",
    "RenderSliverFixedExtentList",
    "RenderSliverGrid",
    "RenderSliverPadding",
    "RenderSliverMainAxisGroup",
    "RenderSliverToBoxAdapter",
    "RenderSliverFillViewport",
    "RenderSliverFillRemaining",
    "RenderSliverFillRemainingAndOverscroll",
    "RenderSliverFillRemainingWithScrollable",
    "RenderSliverIgnorePointer",
    "RenderSliverList",
    "RenderSliverOffstage",
    "RenderSliverOpacity",
    "RenderSliverAnimatedOpacity",
    "RenderViewport",
    "RenderShrinkWrappingViewport",
    "RenderWrap",
    "RenderIntrinsicWidth",
    "RenderIntrinsicHeight",
    "RenderConstrainedOverflowBox",
    "RenderSizedOverflowBox",
    "RenderConstraintsTransformBox",
    "RenderRotatedBox",
    "RenderAnimatedSize",
    "RenderSliverScrollingPersistentHeader",
    "RenderSliverPinnedPersistentHeader",
    "RenderSliverFloatingPersistentHeader",
    "RenderSliverFloatingPinnedPersistentHeader",
];

fn loose(max: f64) -> BoxConstraints {
    BoxConstraints::new(0.0, max, 0.0, max)
}

// ============================================================================
// Intrinsics test doubles (RenderIntrinsicWidth / RenderIntrinsicHeight)
// ============================================================================

/// Leaf reporting independently configurable min/max intrinsic width and
/// height, regardless of the queried extent — a test-only fixture.
///
/// Lays itself out at the midpoint of its min/max on each axis, clamped to
/// whatever constraints its parent hands it (`constraints.constrain(...)`).
#[derive(Debug, Clone, Copy)]
struct RenderTestBox {
    min_width: f64,
    max_width: f64,
    min_height: f64,
    max_height: f64,
}

impl RenderTestBox {
    fn new(min_width: f64, max_width: f64, min_height: f64, max_height: f64) -> Self {
        Self {
            min_width,
            max_width,
            min_height,
            max_height,
        }
    }
}

impl flui_foundation::Diagnosticable for RenderTestBox {}

impl RenderBox for RenderTestBox {
    type Arity = flui_foundation::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<
            '_,
            flui_foundation::Leaf,
            Self::ParentData,
        >,
    ) -> Size {
        let midpoint = Size::new(
            self.min_width + (self.max_width - self.min_width) / 2.0,
            self.min_height + (self.max_height - self.min_height) / 2.0,
        );
        ctx.constraints().constrain(midpoint)
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<
            '_,
            flui_foundation::Leaf,
            Self::ParentData,
        >,
    ) -> bool {
        false
    }

    fn compute_min_intrinsic_width(&self, _height: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.min_width
    }

    fn compute_max_intrinsic_width(&self, _height: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.max_width
    }

    fn compute_min_intrinsic_height(&self, _width: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.min_height
    }

    fn compute_max_intrinsic_height(&self, _width: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.max_height
    }
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
            context.layout_child("header", BoxConstraints::tight(Size::new(size.width, 20.0)));
            context.position_child("header", Offset::ZERO);
        }
        if context.has_child("body") {
            context.layout_child("body", BoxConstraints::tight(Size::new(70.0, 30.0)));
            context.position_child("body", Offset::new(10.0, 25.0));
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

/// A request-strategy fixed-extent list over `children`, each seeded with the
/// logical index of its position — the parent data the element tree would
/// have stamped at adoption.
fn fixed_extent_list(item_extent: f64, children: Vec<TreeNode>) -> TreeNode {
    let count = children.len();
    let mut node = sliver_node(RenderSliverFixedExtentList::new(item_extent, count));
    for (index, child) in children.into_iter().enumerate() {
        node = node.child(
            child.with_parent_data_seed(ParentDataSeed::SliverMultiBoxAdaptor(
                SliverMultiBoxAdaptorParentData::new(index),
            )),
        );
    }
    node
}

/// A request-strategy grid over `children`, each seeded with the logical
/// index of its position — the parent data the element tree would have
/// stamped at adoption.
fn grid_list(delegate: Arc<dyn SliverGridDelegate>, children: Vec<TreeNode>) -> TreeNode {
    let count = children.len();
    let mut node = sliver_node(RenderSliverGrid::new(delegate, count));
    for (index, child) in children.into_iter().enumerate() {
        node = node.child(
            child.with_parent_data_seed(ParentDataSeed::SliverMultiBoxAdaptor(
                SliverMultiBoxAdaptorParentData::new(index),
            )),
        );
    }
    node
}

fn viewport_with_scroll(offset: f64, sliver: TreeNode) -> TreeNode {
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

fn harness_sized_box_forces_dimensions() {
    let run = RenderTester::mount(box_node(RenderSizedBox::fixed(80.0, 60.0)))
        .with_constraints(loose(200.0))
        .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(80.0, 60.0));
    assert_descendant_properties(&run.diagnostics(), "RenderSizedBox", &["width", "height"]);
}

fn harness_colored_box_self_describes_and_paints() {
    let run = RenderTester::mount(box_node(RenderColoredBox::red(50.0, 50.0)))
        .with_size(Size::new(100.0, 100.0))
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

fn harness_render_error_box_fills_bounded_constraints_and_paints() {
    let mut run = RenderTester::mount(box_node(RenderErrorBox::new("boom", None)))
        .with_size(Size::new(100.0, 60.0))
        .run_frame();
    assert!(run.painted(), "an error box must paint something visible");
    let tree = run.diagnostics();
    let node = tree.find_descendant("RenderErrorBox").expect("error box");
    assert_has_committed_size(node);
    assert_eq!(
        run.descendant_property("RenderErrorBox", "message")
            .as_deref(),
        Some("boom"),
        "the caught message reaches diagnostics in every build"
    );
    assert_eq!(error_box_size(&run), (100.0, 60.0));
    // The message is shaped at layout, through the realm's text context, and
    // painted in debug builds only.
    let paints = |run: &flui_rendering::testing::FrameRun, message: &str| {
        run.display_commands()
            .iter()
            .any(|command| command.line.contains("Paragraph") && command.line.contains(message))
    };
    assert_eq!(
        paints(&run, "boom"),
        cfg!(debug_assertions),
        "a debug build paints the message, a release build withholds it"
    );

    // A new message is shaped at the next layout: the impact `set_error`
    // reports must schedule one, or the old paragraph is painted again.
    let root = run.root();
    flui_rendering::testing::update_render_object::<RenderErrorBox, _>(
        run.owner_mut(),
        root,
        |error_box| error_box.set_error("bang", None),
    );
    run.pump();
    assert_eq!(
        (paints(&run, "bang"), paints(&run, "boom")),
        (cfg!(debug_assertions), false),
        "the next frame paints the new message and not the old one"
    );
}

/// The committed size of the mounted `RenderErrorBox`, read back from its
/// diagnostics (`Size { width: 200px, height: 48px }`).
fn error_box_size(run: &flui_rendering::testing::FrameRun) -> (f64, f64) {
    let raw = run
        .descendant_property("RenderErrorBox", "size")
        .expect("committed size");
    let field = |name: &str| -> f64 {
        let start = raw
            .find(name)
            .unwrap_or_else(|| panic!("{name} in {raw:?}"))
            + name.len();
        raw[start..]
            .trim_start_matches([':', ' '])
            .trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.')
            .split(|c: char| !c.is_ascii_digit() && c != '.')
            .next()
            .and_then(|n| n.parse::<f64>().ok())
            .unwrap_or_else(|| panic!("{name} in {raw:?}"))
    };
    (field("width"), field("height"))
}

fn harness_render_error_box_falls_back_to_a_finite_extent_on_an_unbounded_axis() {
    // A lazy list's main axis is unbounded: the box must take a finite row,
    // not the whole scroll extent.
    let run = RenderTester::mount(box_node(RenderErrorBox::new("boom", None)))
        .with_constraints(BoxConstraints::new(0.0, 200.0, 0.0, f64::INFINITY))
        .run_frame();
    assert_eq!(error_box_size(&run), (200.0, ERROR_BOX_FALLBACK_EXTENT));
}

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
    // CustomPaint sizes to its child (20x10 `RenderColoredBox`), so both
    // painters must be invoked with THAT size, not the Size::ZERO preferred
    // size given at construction (background/foreground painters share the
    // node's one committed `size`).
    assert!(
        rects
            .iter()
            .all(|line| line.contains("(0.00,0.00 20.00x10.00)")),
        "background, child, and foreground must all paint at the node's \
         committed 20x10 size; commands:\n{}",
        painted.join("\n"),
    );
}

/// A painter that calls `canvas.save()` without a matching `restore()` before
/// `paint()` returns must poison the paint phase rather than silently
/// corrupting the canvas save/restore stack.
///
/// The check is the `debug_assert_eq!` in `paint_with_painter`
/// (`crates/flui-objects/src/proxy/custom_paint.rs`), which raises a Rust
/// panic; the pipeline's `catch_unwind` wrapper
/// (`crates/flui-rendering/src/pipeline/owner/paint.rs`) converts ANY paint
/// panic into `RenderError::Poisoned { render_object, phase }` and discards
/// the panic payload — so the specific "must pair every canvas.save()..."
/// message has no observable equivalent here. What IS verified: the imbalance
/// is detected and the paint phase is rejected rather than producing a broken
/// display.
// Debug-only: the imbalance is detected by a `debug_assert_eq!`
// (`flui-rendering/src/context/paint_cx.rs`), so in release nothing panics,
// nothing is poisoned, and this test's own `panic!` arm fires instead.
#[cfg(debug_assertions)]
fn harness_custom_paint_unbalanced_save_poisons_the_paint_phase() {
    #[derive(Debug)]
    struct UnbalancedSavePainter;

    impl CustomPainter for UnbalancedSavePainter {
        fn paint(&self, canvas: &mut Canvas, _size: Size) {
            canvas.save();
            // Deliberately no matching `restore()`.
        }

        fn should_repaint(&self, _old_delegate: &dyn CustomPainter) -> bool {
            true
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    let result = RenderTester::mount(box_node(RenderCustomPaint::new(
        Some(Arc::new(UnbalancedSavePainter)),
        None,
        Size::new(10.0, 10.0),
    )))
    .with_constraints(loose(200.0))
    .try_run_frame();

    match result {
        Err(flui_rendering::RenderError::Poisoned {
            render_object,
            phase,
        }) => {
            assert!(
                render_object.contains("RenderCustomPaint"),
                "render_object name must identify the offending node; got {render_object}",
            );
            assert_eq!(
                phase,
                flui_rendering::PoisonPhase::Paint,
                "phase tag must identify the paint phase",
            );
        }
        other => {
            panic!("expected RenderError::Poisoned from the unbalanced save(), got {other:?}")
        }
    }
}

fn harness_listener_passes_layout_through_and_attaches_handler() {
    // A lane-registered no-op target — the harness verifies its identity
    // reaches the hit entry (the pipeline wiring); that it FIRES end-to-end is
    // covered by the Listener widget's dispatch test.
    let lane = flui_interaction::InteractionLane::try_new().expect("interaction lane");
    let target = lane.enter(|| {
        lane.dispatch_handle()
            .register_pointer(|_event| {})
            .expect("register no-op pointer target")
    });
    let run = RenderTester::mount(
        // DeferToChild over a hittable ColoredBox: the listener registers when
        // the child is hit.
        box_node(RenderListener::new(
            Some(target),
            HitTestBehavior::DeferToChild,
        ))
        .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    // Layout is a pure pass-through: the listener sizes to its 40×40 child.
    assert_eq!(run.box_geometry(run.root()), Size::new(40.0, 40.0));

    // A pointer landing on the child hits the listener (it registers itself in
    // the leaf-first path alongside its child), and its hit entry carries the
    // data-only target identity supplied by the render object.
    assert!(
        run.hit(20.0, 20.0).contains(&run.root()),
        "the listener registers itself in the hit path",
    );
    let mut result = HitTestResult::new();
    run.pipeline()
        .hit_test(Offset::new(20.0, 20.0), &mut result);
    assert!(
        result
            .path()
            .iter()
            .any(|entry| entry.pointer_target == Some(target)),
        "the listener's hit entry must carry its pointer target:\n{}",
        run.diagnostics(),
    );
}

fn harness_mouse_region_uses_one_tracker_target_for_hover_enter_and_exit() {
    let hovers = Rc::new(Cell::new(0));
    let enters = Rc::new(Cell::new(0));
    let exits = Rc::new(Cell::new(0));
    let lane = flui_interaction::InteractionLane::try_new().expect("interaction lane");
    let mouse_target = lane.enter(|| {
        let handle = lane.dispatch_handle();
        let hover_counter = Rc::clone(&hovers);
        let enter_counter = Rc::clone(&enters);
        let exit_counter = Rc::clone(&exits);
        handle
            .register_mouse_region(MouseRegionCallbacks {
                on_enter: Some(Rc::new(move |_device, _position| {
                    enter_counter.set(enter_counter.get() + 1);
                })),
                on_exit: Some(Rc::new(move |_device, _position| {
                    exit_counter.set(exit_counter.get() + 1);
                })),
                on_hover: Some(Rc::new(move |_device, _position| {
                    hover_counter.set(hover_counter.get() + 1);
                })),
            })
            .expect("register mouse-region target")
    });

    let mut region = RenderMouseRegion::new();
    region.set_mouse_region_target(Some(mouse_target));

    let run = RenderTester::mount(box_node(region))
        .with_constraints(BoxConstraints::tight(Size::new(60.0, 30.0)))
        .run_frame();

    let mut inside = HitTestResult::new();
    let inside_position = Offset::new(10.0, 10.0);
    run.pipeline().hit_test(inside_position, &mut inside);
    let tracker = MouseTracker::new();
    tracker.add_device(
        0,
        flui_interaction::events::PointerType::Mouse,
        Offset::ZERO,
    );

    // `update_with_motion` (enter/exit/cursor tracking) and `dispatch_hover`
    // (on_hover) are two independent dispatch paths that both resolve the
    // SAME `MouseRegionTarget` -- one tracker target really does serve all
    // three callbacks, just not through one call. `make_move_event` defaults
    // `buttons` to a button held (it targets contact-motion tests), so a
    // genuine hover-shaped move needs buttons cleared explicitly.
    let mut hover_event = flui_interaction::events::make_move_event(
        inside_position,
        flui_interaction::events::PointerType::Mouse,
    );
    let flui_interaction::events::PointerEvent::Move(hover_update) = &mut hover_event else {
        unreachable!("make_move_event always returns PointerEvent::Move")
    };
    hover_update.current.buttons = flui_interaction::events::PointerButtons::new();

    lane.enter(|| {
        tracker.update_with_motion(&hover_event, PointerMotionKind::Hover, &inside);
    });
    assert_eq!(enters.get(), 1, "first tracker update enters the region");
    assert_eq!(
        hovers.get(),
        0,
        "on_hover is dispatch_hover's concern, not update_with_motion's",
    );

    lane.enter(|| {
        assert!(tracker.dispatch_hover(&hover_event, &inside).is_none());
    });
    assert_eq!(
        hovers.get(),
        1,
        "dispatch_hover resolves the same tracker target enter/exit used",
    );

    let contact_event = flui_interaction::events::make_move_event(
        inside_position,
        flui_interaction::events::PointerType::Mouse,
    );
    lane.enter(|| {
        tracker.update_with_motion(&contact_event, PointerMotionKind::Contact, &inside);
        assert!(tracker.dispatch_hover(&contact_event, &inside).is_none());
    });
    assert_eq!(
        hovers.get(),
        1,
        "a buttons-held move refreshes tracking but must not invoke on_hover",
    );

    let mut outside = HitTestResult::new();
    let outside_position = Offset::new(80.0, 10.0);
    run.pipeline().hit_test(outside_position, &mut outside);
    let outside_event = flui_interaction::events::make_move_event(
        outside_position,
        flui_interaction::events::PointerType::Mouse,
    );
    lane.enter(|| {
        lane.dispatch_handle()
            .unregister_mouse_region(mouse_target)
            .expect("unregister mouse target after prior annotation was resolved");
        tracker.update_with_motion(&outside_event, PointerMotionKind::Hover, &outside);
    });
    assert_eq!(
        exits.get(),
        1,
        "tracker must retain the prior annotation long enough to fire exit",
    );
}

fn harness_image_paints_placeholder_frame() {
    let run = RenderTester::mount(box_node(RenderImage::new(
        Size::new(50.0, 50.0),
        ImageFit::Cover,
        ImageAlignment::Center,
    )))
    .with_size(Size::new(100.0, 100.0))
    .run_frame();

    assert!(run.painted());
}

fn harness_paragraph_paints_text_frame() {
    let run = RenderTester::mount(box_node(RenderParagraph::new(
        TextSpan::new("paint me"),
        TextDirection::Ltr,
    )))
    .with_size(Size::new(200.0, 100.0))
    .run_frame();

    assert!(run.painted());
}

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
    assert_eq!(size.width, 160.0);
    assert!(size.height >= 18.0);

    let commands = run.display_commands();
    assert!(
        commands
            .iter()
            .any(|command| command.line.contains("Paragraph") && command.line.contains("edit me")),
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

/// The selection highlight paints **behind** the glyphs: it runs before the
/// text is painted.
///
/// The caret is hidden so the only `DrawRect` in the frame is the highlight;
/// the assertion is a strict index comparison, not "a rect exists somewhere".
///
/// Red-check: move `self.paint_selection(ctx)` after `self.painter.paint(..)`
/// in `RenderEditable::paint` — the indices invert and this fails.
fn harness_editable_paints_the_selection_behind_the_glyphs() {
    let run = RenderTester::mount(box_node(
        RenderEditable::new(TextSpan::new("edit me"), TextDirection::Ltr)
            .with_show_caret(false)
            .with_selection(Some(0..4))
            .with_selection_color(Color::BLUE),
    ))
    .with_constraints(loose(160.0))
    .run_frame();

    let commands = run.display_commands();
    let highlight = commands
        .iter()
        .position(|command| command.line.contains("DrawRect"))
        .unwrap_or_else(|| panic!("a selection must paint a rect; commands: {commands:#?}"));
    let glyphs = commands
        .iter()
        .position(|command| command.line.contains("Paragraph"))
        .unwrap_or_else(|| panic!("the text must paint; commands: {commands:#?}"));

    assert!(
        highlight < glyphs,
        "the highlight must precede the glyphs it sits behind, got \
         highlight at {highlight} and glyphs at {glyphs}; commands: {commands:#?}"
    );
}

// ------------------------------------------------------------------------
// Composing-region underline (ADR-0030)
// ------------------------------------------------------------------------

/// The composing-region underline paints at exactly the box
/// `get_boxes_for_selection` reports for that byte range — the byte-offset
/// agreement pin: a MULTIBYTE (CJK) range specifically catches a char-count
/// vs byte-count mixup in the composing-range plumbing, which a pure-ASCII
/// range cannot distinguish (every ASCII char is exactly one byte).
///
/// Red-check: write this test before `RenderEditable::paint`'s underline
/// branch exists (or with `composing_range` never wired in) — it fails
/// because no `DrawRect` command matches the expected rect at all (only the
/// `Paragraph` command is present).
fn harness_editable_composing_underline_paints_at_the_exact_multibyte_box() {
    // "abc" (3 ASCII bytes) + "你好" (two 3-byte CJK chars = 6 bytes) + "def".
    let text = "abc你好def";
    let composing_start = "abc".len();
    let composing_end = composing_start + "你好".len();

    let run = RenderTester::mount(box_node(
        RenderEditable::new(TextSpan::new(text), TextDirection::Ltr)
            .with_composing_range(Some(composing_start..composing_end))
            .with_show_caret(false),
    ))
    .with_constraints(loose(400.0))
    .run_frame();

    let editable = run
        .owner()
        .render_tree()
        .get(run.root())
        .expect("root render id must be live")
        .as_box()
        .expect("root is a box node")
        .render_object()
        .downcast_ref::<RenderEditable>()
        .expect("root is a RenderEditable");

    let boxes = editable
        .painter()
        .get_boxes_for_selection(composing_start, composing_end);
    assert_eq!(
        boxes.len(),
        1,
        "a single-line contiguous CJK range must produce exactly one box"
    );
    let expected_box = boxes[0].rect;
    let baseline = editable
        .compute_distance_to_actual_baseline(TextBaseline::Alphabetic)
        .expect("layout ran, so a baseline must be available");
    // Mirrors `RenderEditable`'s own private `underline_rect_for_box` clamp —
    // baseline + 1px gap, clamped inside the box's vertical span.
    let top = expected_box.top();
    let max_top = (expected_box.bottom() - 1.0).max(top);
    let expected_top = (baseline + 1.0).clamp(top, max_top);

    let commands = run.display_commands();
    let expected_rect_fragment = format!(
        "rect=({:.2},{:.2} {:.2}x{:.2})",
        expected_box.left(),
        expected_top,
        expected_box.width(),
        1.0
    );
    assert!(
        commands
            .iter()
            .any(|c| c.line.contains("DrawRect") && c.line.contains(&expected_rect_fragment)),
        "the underline must paint exactly at the CJK composing range's box \
         (byte-offset agreement, not a char-offset mixup); expected fragment \
         {expected_rect_fragment:?}; commands: {commands:#?}"
    );
}

// ============================================================================
// Single-child box proxies
// ============================================================================

fn harness_padding_deflates_child_offset() {
    let run = RenderTester::mount(
        box_node(RenderPadding::all(12.0))
            .child(box_node(RenderColoredBox::red(30.0, 30.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.offset(run.id("child")), Offset::new(12.0, 12.0));
    assert!(
        run.descendant_property("RenderPadding", "padding")
            .is_some()
    );
}

fn harness_custom_single_child_layout_positions_child_with_delegate() {
    let delegate = custom_single_child_delegate(
        Size::new(120.0, 80.0),
        BoxConstraints::tight(Size::new(30.0, 20.0)),
        Offset::new(70.0, 50.0),
    );
    let run = RenderTester::mount(
        box_node(RenderCustomSingleChildLayoutBox::new(delegate))
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(120.0, 80.0),
        "parent size must come from delegate.get_size constrained by incoming constraints",
    );
    assert_eq!(
        run.box_geometry(run.id("child")),
        Size::new(30.0, 20.0),
        "child must be laid out under delegate.get_constraints_for_child",
    );
    assert_eq!(run.offset(run.id("child")), Offset::new(70.0, 50.0));
    assert_eq!(run.hit_first(75.0, 55.0), Some(run.id("child")));
    assert_eq!(run.hit(10.0, 10.0), [] as [flui_foundation::RenderId; 0]);
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

fn harness_custom_multi_child_layout_positions_children_by_layout_id() {
    let delegate = custom_multi_child_delegate(Size::new(120.0, 90.0));
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
        Size::new(120.0, 90.0),
        "parent size must come from delegate.get_size constrained by incoming constraints",
    );
    assert_eq!(
        run.box_geometry(run.id("header")),
        Size::new(120.0, 20.0),
        "header receives tight constraints from the delegate",
    );
    assert_eq!(
        run.box_geometry(run.id("body")),
        Size::new(70.0, 30.0),
        "body receives different tight constraints from the delegate",
    );
    assert_eq!(run.offset(run.id("header")), Offset::ZERO);
    assert_eq!(run.offset(run.id("body")), Offset::new(10.0, 25.0));
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

fn harness_center_centers_child() {
    let run = RenderTester::mount(
        box_node(RenderCenter::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_size(Size::new(100.0, 100.0))
    .run_layout();

    assert_eq!(run.offset(run.id("child")), Offset::new(30.0, 30.0));
    assert!(run.diagnostics().find_descendant("RenderCenter").is_some());
}

fn harness_baseline_positions_text_at_offset() {
    let mut run = RenderTester::mount(
        box_node(RenderBaseline::new(TextBaseline::Alphabetic, 0.0)).child(
            box_node(RenderParagraph::new(
                TextSpan::new("Ag"),
                TextDirection::Ltr,
            ))
            .label("text"),
        ),
    )
    .with_size(Size::new(200.0, 100.0))
    .run_layout();

    let tree = run.diagnostics();
    assert_has_committed_size(
        tree.find_descendant("RenderBaseline")
            .expect("RenderBaseline"),
    );
    assert_descendant_properties(&tree, "RenderBaseline", &["baseline"]);
    let constraints = BoxConstraints::loose(Size::new(200.0, 100.0));
    let baseline = run
        .dry_baseline(run.root(), constraints, TextBaseline::Alphabetic)
        .expect("paragraph reports a dry baseline");
    assert_eq!(baseline, 0.0);
}

/// Baselines of different kinds.
///
/// A leaf probe reports independent alphabetic/ideographic offsets. With the box's
/// own kind set to Alphabetic:
/// - a same-kind query cancels to the configured `baseline_offset` alone
///   (`1.0 + 50 - 50 = 1.0`);
/// - a cross-kind query adds the requested/own child delta
///   (`1.0 + 60 - 50 = 11.0`), matching `compute_dry_baseline`'s
///   `baseline_offset + requested - own` formula.
///
/// After the probe's offsets are cleared to `None` and the child is marked
/// layout-dirty (clearing the baseline cache), a relayout must
/// recompute both queries to `None` rather than serve the prior 1.0/11.0 —
/// a stale-value regression (whether the dry-baseline query is memoized per
/// call or always recomputed live, the observable contract — fresh state in,
/// fresh answer out — must hold).
fn harness_baseline_dry_baseline_recomputes_per_kind_offsets_after_relayout() {
    use flui_foundation::Leaf;
    use flui_rendering::context::{BoxDryBaselineCtx, BoxDryLayoutCtx, BoxLayoutContext};
    use flui_rendering::parent_data::BoxParentData;

    /// Leaf render object with independently settable per-kind baseline
    /// offsets.
    #[derive(Debug)]
    struct BaselineOffsetProbe {
        box_size: Size,
        alphabetic_offset: Option<f64>,
        ideographic_offset: Option<f64>,
    }

    impl flui_foundation::Diagnosticable for BaselineOffsetProbe {
        fn debug_fill_properties(&self, _properties: &mut flui_foundation::DiagnosticsBuilder) {}
    }

    impl RenderBox for BaselineOffsetProbe {
        type Arity = Leaf;
        type ParentData = BoxParentData;

        fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
            ctx.constraints().constrain(self.box_size)
        }

        fn compute_dry_layout(
            &self,
            constraints: BoxConstraints,
            _ctx: &mut BoxDryLayoutCtx<'_>,
        ) -> Size {
            constraints.constrain(self.box_size)
        }

        fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f64> {
            match baseline {
                TextBaseline::Alphabetic => self.alphabetic_offset,
                TextBaseline::Ideographic => self.ideographic_offset,
            }
        }

        fn compute_dry_baseline(
            &self,
            _constraints: BoxConstraints,
            baseline: TextBaseline,
            _ctx: &mut BoxDryBaselineCtx<'_>,
        ) -> Option<f64> {
            match baseline {
                TextBaseline::Alphabetic => self.alphabetic_offset,
                TextBaseline::Ideographic => self.ideographic_offset,
            }
        }
    }

    let mut run = RenderTester::mount(
        box_node(RenderBaseline::new(TextBaseline::Alphabetic, 1.0)).child(
            box_node(BaselineOffsetProbe {
                box_size: Size::new(100.0, 100.0),
                alphabetic_offset: Some(50.0),
                ideographic_offset: Some(60.0),
            })
            .label("child"),
        ),
    )
    .with_constraints(loose(1000.0))
    .run_layout();

    let root = run.root();
    let constraints = loose(1000.0);

    assert_eq!(
        run.dry_baseline(root, constraints, TextBaseline::Alphabetic),
        Some(1.0),
        "same-kind dry baseline must cancel to the configured baseline_offset",
    );
    assert_eq!(
        run.dry_baseline(root, constraints, TextBaseline::Ideographic),
        Some(11.0),
        "cross-kind dry baseline must be baseline_offset + requested - own",
    );

    run.update::<BaselineOffsetProbe>(run.id("child"), |probe| {
        probe.alphabetic_offset = None;
        probe.ideographic_offset = None;
    });
    run.relayout();

    assert_eq!(
        run.dry_baseline(root, constraints, TextBaseline::Alphabetic),
        None,
        "dry baseline must recompute to None once the child reports no real \
         baseline, not serve a stale cached value",
    );
    assert_eq!(
        run.dry_baseline(root, constraints, TextBaseline::Ideographic),
        None,
        "cross-kind dry baseline must also recompute to None after relayout",
    );
}

/// Leaf render object with independently settable per-kind baseline offsets
/// and a fixed size — the deterministic (size, baseline) pair the flex
/// cross-extent tests need.
#[derive(Debug)]
struct SizedBaselineProbe {
    box_size: Size,
    alphabetic_offset: Option<f64>,
}

impl flui_foundation::Diagnosticable for SizedBaselineProbe {
    fn debug_fill_properties(&self, _properties: &mut flui_foundation::DiagnosticsBuilder) {}
}

impl RenderBox for SizedBaselineProbe {
    type Arity = flui_foundation::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<
            '_,
            flui_foundation::Leaf,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> Size {
        ctx.constraints().constrain(self.box_size)
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut flui_rendering::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        constraints.constrain(self.box_size)
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f64> {
        match baseline {
            TextBaseline::Alphabetic => self.alphabetic_offset,
            TextBaseline::Ideographic => None,
        }
    }

    fn compute_dry_baseline(
        &self,
        _constraints: BoxConstraints,
        baseline: TextBaseline,
        _ctx: &mut flui_rendering::context::BoxDryBaselineCtx<'_>,
    ) -> Option<f64> {
        match baseline {
            TextBaseline::Alphabetic => self.alphabetic_offset,
            TextBaseline::Ideographic => None,
        }
    }
}

/// A `RenderIgnoreBaseline` reports no baseline of its own, and its wrapped
/// child neither aligns to nor grows a baseline-aligned row: the row falls
/// back to the remaining children's ascent/descent stack, and the wrapped
/// child sits flush at the cross start.
fn harness_ignore_baseline_hides_its_child_from_a_baseline_row() {
    let run = RenderTester::mount(
        box_node(
            RenderFlex::row()
                .with_cross_axis_alignment(CrossAxisAlignment::Baseline)
                .with_text_baseline(TextBaseline::Alphabetic),
        )
        .child(
            box_node(RenderIgnoreBaseline::new())
                .label("ignored")
                .child(
                    box_node(SizedBaselineProbe {
                        box_size: Size::new(100.0, 50.0),
                        alphabetic_offset: Some(40.0),
                    })
                    .label("hidden_child"),
                ),
        )
        .child(
            box_node(SizedBaselineProbe {
                box_size: Size::new(100.0, 60.0),
                alphabetic_offset: Some(10.0),
            })
            .label("visible"),
        ),
    )
    .with_constraints(loose(1000.0))
    .run_layout();

    assert_eq!(
        run.offset(run.id("ignored")).dy,
        0.0,
        "the wrapped child is not baseline-aligned — it sits at the cross start",
    );
    assert_eq!(
        run.offset(run.id("visible")).dy,
        0.0,
        "with only one baseline left in the row, that child defines the common \
         baseline and is not shifted either",
    );
    assert_eq!(
        run.box_geometry(run.root()).height,
        60.0,
        "the row is sized by the surviving ascent (10) + descent (50) and the \
         raw heights — the hidden child's 40px ascent must not stretch it to 90",
    );
}

fn harness_aspect_ratio_enforces_ratio() {
    // Loose constraints let `_apply_aspect_ratio` honour the ratio; tight
    // constraints return `constraints.smallest()` unchanged.
    let run = RenderTester::mount(
        box_node(RenderAspectRatio::new(AspectRatioFactor::new_unchecked(
            2.0,
        )))
        .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    let size = run.box_geometry(run.root());
    assert!((size.width / size.height - 2.0).abs() < 0.01);
    assert_eq!(
        run.descendant_property_f64("RenderAspectRatio", "aspect_ratio"),
        Some(2.0)
    );
}

fn harness_aspect_ratio_unbounded_minima_remain_finite() {
    fn check(ratio: AspectRatioFactor, constraints: BoxConstraints, expected: Size) {
        let mut run = RenderTester::mount(
            box_node(RenderAspectRatio::new(ratio))
                .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
        )
        .with_constraints(constraints)
        .run_layout();
        assert_eq!(run.box_geometry(run.root()), expected);
        assert_eq!(run.box_geometry(run.id("child")), expected);
        assert_eq!(run.dry_layout(run.root(), constraints), expected);
    }
    fn ratio(value: f64) -> AspectRatioFactor {
        AspectRatioFactor::new(value).expect("valid ratio")
    }
    fn height_minimum() {
        check(
            ratio(2.0),
            BoxConstraints::new(40.0, f64::INFINITY, 30.0, f64::INFINITY),
            Size::new(60.0, 30.0),
        );
    }
    fn width_minimum() {
        check(
            ratio(2.0),
            BoxConstraints::new(80.0, f64::INFINITY, 30.0, f64::INFINITY),
            Size::new(80.0, 40.0),
        );
    }
    fn zero_minima() {
        check(ratio(2.0), BoxConstraints::UNCONSTRAINED, Size::ZERO);
    }
    fn tiny_ratio() {
        // MAX = (2 - 2^-52) * 2^1023: multiplying by 2^-1022
        // gives the representable predecessor of 4, without overflow.
        check(
            ratio(f64::MIN_POSITIVE),
            BoxConstraints::new(0.0, 100.0, 0.0, f64::INFINITY),
            Size::new(f64::from_bits(0x400f_ffff_ffff_ffff), f64::MAX),
        );
    }
    fn huge_ratio() {
        check(
            ratio(f64::MAX),
            BoxConstraints::new(0.0, f64::INFINITY, 0.0, 100.0),
            Size::new(f64::MAX, 1.0),
        );
    }
    fn unrepresentable_ratio_with_minimum() {
        check(
            ratio(f64::MIN_POSITIVE),
            BoxConstraints::new(100.0, f64::INFINITY, 30.0, f64::INFINITY),
            Size::new(100.0, f64::MAX),
        );
    }
    fn tight_constraints() {
        check(
            ratio(2.0),
            BoxConstraints::tight(Size::new(40.0, 30.0)),
            Size::new(40.0, 30.0),
        );
    }
    fn unrepresentable_inverse() {
        // The reciprocal of the smallest positive value is not representable.
        check(
            ratio(f64::from_bits(1)).inverse(),
            BoxConstraints::new(40.0, 100.0, 30.0, 100.0),
            Size::new(40.0, 30.0),
        );
    }
    run_family(
        "aspect ratio finite geometry",
        &[
            ("height_minimum", height_minimum),
            ("width_minimum", width_minimum),
            ("zero_minima", zero_minima),
            ("tiny_ratio", tiny_ratio),
            ("huge_ratio", huge_ratio),
            (
                "unrepresentable_ratio_with_minimum",
                unrepresentable_ratio_with_minimum,
            ),
            ("tight_constraints", tight_constraints),
            ("unrepresentable_inverse", unrepresentable_inverse),
        ],
    );
}

fn harness_aspect_ratio_infinite_minima_reject_without_panicking() {
    fn check(additional: BoxConstraints) {
        let mut run = RenderTester::mount(
            box_node(RenderConstrainedBox::new(additional))
                .child(box_node(RenderAspectRatio::new(AspectRatioFactor::SQUARE)).label("ratio")),
        )
        .with_constraints(BoxConstraints::UNCONSTRAINED)
        .run_layout();
        let ratio = run.id("ratio");
        // Descendant failures are contained as stand-in geometry. Re-arm the
        // affected node and use the same production driver directly to inspect
        // its typed rejection rather than the parent's contained result.
        run.owner_mut().mark_needs_layout(ratio);
        let error = run
            .owner_mut()
            .layout_dirty_root(ratio, additional)
            .expect_err("infinite minima cannot admit finite geometry");
        assert!(
            matches!(
                error,
                flui_rendering::RenderError::InvalidGeometry {
                    render_object,
                    ..
                } if render_object == std::any::type_name::<RenderAspectRatio>()
            ),
            "expected typed geometry rejection, got {error:?}"
        );

        // A real parent update must make the same child usable again.
        let root = run.root();
        run.update::<RenderConstrainedBox>(root, |parent| {
            // LayoutRun::update marks this node for layout.
            let _ = parent.set_additional_constraints(BoxConstraints::tight(Size::new(40.0, 30.0)));
        });
        run.relayout();
        assert_eq!(run.box_geometry(ratio), Size::new(40.0, 30.0));
    }
    fn both_axes() {
        check(BoxConstraints::expand());
    }
    fn width_only() {
        check(BoxConstraints::tight_for(Some(f64::INFINITY), None));
    }
    run_family(
        "aspect ratio infinite minimum rejection",
        &[("both_axes", both_axes), ("width_only", width_only)],
    );
}

fn harness_constrained_box_enforces_minimums() {
    let extra = BoxConstraints::new(100.0, f64::INFINITY, 100.0, f64::INFINITY);
    let run = RenderTester::mount(
        box_node(RenderConstrainedBox::new(extra))
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    let child = run.box_geometry(run.id("child"));
    assert!(child.width >= 100.0);
    assert!(child.height >= 100.0);
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderConstrainedBox",
        &["additional_constraints"],
    );
}

// ════════════════════════════════════════════════════════════════════════
// RenderContainer — the collapsed form of the Container widget stack
// ════════════════════════════════════════════════════════════════════════

/// One configuration of the stack `RenderContainer` collapses, used by
/// [`assert_container_matches_stack`].
#[derive(Clone)]
struct ContainerStackCase {
    case: &'static str,
    margin: EdgeInsets,
    extra: Option<BoxConstraints>,
    /// `None` means padding was never set — the container stack inserts no
    /// `Padding` level at all in that case, distinct from
    /// `Some(EdgeInsets::ZERO)`, which still gets a real (zero-inset) level
    /// that gates a hit-test the way any other level does. Collapsing the two
    /// into a bare `EdgeInsets` is exactly the bug this case exists to catch,
    /// so the composed tree below must actually omit the level when this is
    /// `None`.
    padding: Option<EdgeInsets>,
    alignment: Option<Alignment>,
    child: Size,
    constraints: BoxConstraints,
    color: Option<Color>,
    decoration: Option<BoxDecoration<f64>>,
    transform: Option<Matrix4>,
    /// When set, the CHILD itself is wrapped in a `RenderTransform` using
    /// this matrix — distinct from `transform` above, which is the
    /// CONTAINER's own outer transform applied to the whole collapsed
    /// fragment. `RenderTransform::hit_test` deliberately does not bound
    /// itself to its own laid-out box (a scaled child visually covering
    /// more than that box must stay hittable across the whole area), so a
    /// case that sets this exercises a child whose hit region can overflow
    /// past whatever box the levels between the margin and the child gate
    /// on — proving those gates still apply ahead of the collapsed node's
    /// own child recursion.
    child_overflow_transform: Option<Matrix4>,
}

/// Asserts that one `RenderContainer` configuration is geometrically
/// indistinguishable from the widget stack it collapses.
///
/// The stack is assembled here out of the individual render objects, each of
/// which already carries its own tests — so this is the reference for the
/// collapse: size, child size, absolute child position, the
/// child hit path, and chrome self-hit (whether *anything* was hit, not only
/// whether the child was) all have to agree.
///
/// `Align` and `ConstrainedBox` appear only when their property is set,
/// as in the conditional stack. The margin `Padding` level is
/// always present (the container inserts one whenever `margin` is
/// set, which every case here does). The INNER `Padding` level — the one
/// `padding` controls — is present only when `padding` is `Some`, even
/// `Some(EdgeInsets::ZERO)`: an explicit zero inset still gets a real
/// (zero-inset) level that gates a hit-test the way any other level does,
/// but an absent `padding` gets no level at all,
/// which is observably different
/// for hit-testing even though the two are geometrically identical. A
/// version of this helper that always inserted a `Padding(EdgeInsets::ZERO)`
/// level regardless would silently endorse that divergence instead of
/// detecting it — which is exactly what an earlier version of this file did.
/// Color, decoration and transform are included when `Some`, so a case that
/// sets them actually compares against `DecoratedBox` / `RenderTransform`
/// rather than omitting those levels.
fn assert_container_matches_stack(spec: ContainerStackCase) {
    const CHILD_COLOR: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
    let ContainerStackCase {
        case,
        margin,
        extra,
        padding,
        alignment,
        child,
        constraints,
        color,
        decoration,
        transform,
        child_overflow_transform,
    } = spec;

    // The CHILD subtree: a plain colored leaf, or that leaf wrapped in a
    // `RenderTransform` when `child_overflow_transform` is set — built
    // identically for both trees, so only the levels between the margin
    // and the child can disagree about whether it stays reachable.
    // `"child"` always labels whatever RenderContainer treats as ITS
    // child — the leaf itself, or the wrapping `RenderTransform` when one
    // is present — so the existing size/position parity checks below (which
    // assume "child" is the node RenderContainer positions directly) still
    // apply unchanged; the leaf gets its own `"leaf"` label underneath.
    let build_child = || -> TreeNode {
        match child_overflow_transform {
            Some(matrix) => box_node(RenderTransform::new(matrix))
                .label("child")
                .child(box_node(RenderColoredBox::new(CHILD_COLOR, child)).label("leaf")),
            None => box_node(RenderColoredBox::new(CHILD_COLOR, child)).label("child"),
        }
    };

    let mut container = RenderContainer::new().with_margin(margin);
    if let Some(padding) = padding {
        container = container.with_padding(padding);
    }
    if let Some(extra) = extra {
        container = container.with_additional_constraints(extra);
    }
    if let Some(alignment) = alignment {
        container = container.with_alignment(alignment);
    }
    if let Some(color) = color {
        container = container.with_color(color);
    }
    if let Some(decoration) = decoration.clone() {
        container = container.with_decoration(decoration);
    }
    if let Some(transform) = transform {
        container = container.with_transform(transform);
    }
    let collapsed = RenderTester::mount(box_node(container).child(build_child()))
        .with_constraints(constraints)
        .run_frame();

    let mut levels = vec!["child"];
    let mut node = build_child();
    if let Some(alignment) = alignment {
        node = box_node(RenderAlign::new(alignment))
            .label("align")
            .child(node);
        levels.push("align");
    }
    if let Some(padding) = padding {
        node = box_node(RenderPadding::new(padding))
            .label("padding")
            .child(node);
        levels.push("padding");
    }
    if let Some(color) = color {
        node = box_node(RenderDecoratedBox::new(BoxDecoration::with_color(color)))
            .label("color")
            .child(node);
        levels.push("color");
    }
    if let Some(decoration) = decoration {
        node = box_node(RenderDecoratedBox::new(decoration))
            .label("decoration")
            .child(node);
        levels.push("decoration");
    }
    if let Some(extra) = extra {
        node = box_node(RenderConstrainedBox::new(extra))
            .label("constraints")
            .child(node);
        levels.push("constraints");
    }
    node = box_node(RenderPadding::new(margin))
        .label("margin")
        .child(node);
    levels.push("margin");
    if let Some(transform) = transform {
        node = box_node(RenderTransform::new(transform))
            .label("transform")
            .child(node);
        levels.push("transform");
    }

    let composed = RenderTester::mount(node)
        .with_constraints(constraints)
        .run_frame();

    assert_eq!(
        collapsed.box_geometry(collapsed.root()),
        composed.box_geometry(composed.root()),
        "[{case}] the collapsed container must size exactly like the stack it replaces"
    );
    assert_eq!(
        collapsed.box_geometry(collapsed.id("child")),
        composed.box_geometry(composed.id("child")),
        "[{case}] the child must be laid out under the same constraints in both trees"
    );

    // Child POSITION parity. The collapsed tree reaches the child in one hop,
    // so its local offset is already absolute; the stack's has to be summed
    // across the levels it spreads that same offset over. A paint transform
    // does not change layout offsets, so it is included in the sum (it
    // contributes zero) and applied only to the hit probes below.
    let origin = levels
        .into_iter()
        .map(|label| composed.offset(composed.id(label)))
        .fold(Offset::ZERO, |acc, step| {
            Offset::new(acc.dx + step.dx, acc.dy + step.dy)
        });
    assert_eq!(
        collapsed.offset(collapsed.id("child")),
        origin,
        "[{case}] the child must land at the same absolute position in both trees"
    );

    let shift = transform
        .and_then(|matrix| matrix.as_translation())
        .map_or(Offset::ZERO, |(dx, dy)| Offset::new(dx, dy));

    // And the hit path must agree at probes that straddle the child's edges,
    // so a shifted child is caught rather than landing inside both windows.
    let child_size = composed.box_geometry(composed.id("child"));
    for (x, y) in [
        (0.0, 0.0),
        (origin.dx + shift.dx - 1.0, origin.dy + shift.dy - 1.0),
        (origin.dx + shift.dx + 1.0, origin.dy + shift.dy + 1.0),
        (
            origin.dx + shift.dx + child_size.width - 1.0,
            origin.dy + shift.dy + child_size.height - 1.0,
        ),
        (
            origin.dx + shift.dx + child_size.width + 1.0,
            origin.dy + shift.dy + child_size.height + 1.0,
        ),
    ] {
        assert_eq!(
            collapsed.hit_first(x, y) == Some(collapsed.id("child")),
            composed.hit_first(x, y) == Some(composed.id("child")),
            "[{case}] child hit disagreement at ({x}, {y})"
        );
    }

    // Chrome self-hit. Comparing only `hit == child` hid a half-open vs
    // inclusive disagreement on the decoration's max edge: the stack's
    // `DecoratedBox` gates with `is_within_own_size` (half-open on the
    // chrome size), while a collapsed node gates on the *outer* size
    // (margin included) and then used inclusive `Rect::contains`.
    let outer = collapsed.box_geometry(collapsed.root());
    let chrome_left = margin.left + shift.dx;
    let chrome_top = margin.top + shift.dy;
    let chrome_right = chrome_left + outer.width - margin.horizontal_total();
    let chrome_bottom = chrome_top + outer.height - margin.vertical_total();
    let chrome_mid_x = chrome_left + (chrome_right - chrome_left) / 2.0;
    let chrome_mid_y = chrome_top + (chrome_bottom - chrome_top) / 2.0;
    for (x, y, why) in [
        (chrome_right, chrome_mid_y, "exclusive max-x of the chrome"),
        (chrome_mid_x, chrome_bottom, "exclusive max-y of the chrome"),
        (
            margin.left * 0.5 + shift.dx,
            chrome_mid_y,
            "inside the margin band (left)",
        ),
        // Mirrored right/bottom probes: an overflowing child
        // (`child_overflow_transform`) that grows toward the right/bottom
        // from a top-left pivot reaches these bands, not the left/top one
        // above — the left-only probe would miss exactly that regression.
        (
            chrome_right + margin.right * 0.5,
            chrome_mid_y,
            "inside the margin band (right)",
        ),
        (
            chrome_mid_x,
            chrome_bottom + margin.bottom * 0.5,
            "inside the margin band (bottom)",
        ),
    ] {
        assert_eq!(
            collapsed.hit_first(x, y).is_some(),
            composed.hit_first(x, y).is_some(),
            "[{case}] chrome hit disagreement at ({x}, {y}) ({why})"
        );
    }
}

/// The load-bearing parity test. Each case makes a different level decide the
/// outcome, so no single level can be dropped without a failure: an alignment
/// that leaves slack, additional constraints that pin the size against a
/// smaller child, a tight incoming constraint, and a minimum on one axis only.
fn harness_container_matches_the_widget_stack_it_collapses() {
    let unbounded = f64::INFINITY;

    assert_container_matches_stack(ContainerStackCase {
        case: "alignment leaves slack in both axes",
        margin: EdgeInsets::all(5.0),
        extra: Some(BoxConstraints::new(80.0, unbounded, 0.0, unbounded)),
        padding: Some(EdgeInsets::all(8.0)),
        alignment: Some(Alignment::BOTTOM_RIGHT),
        child: Size::new(30.0, 20.0),
        constraints: loose(200.0),
        color: None,
        decoration: None,
        transform: None,
        child_overflow_transform: None,
    });

    assert_container_matches_stack(ContainerStackCase {
        case: "tight additional constraints outvote a smaller child",
        margin: EdgeInsets::all(4.0),
        extra: Some(BoxConstraints::tight(Size::new(120.0, 60.0))),
        padding: Some(EdgeInsets::all(6.0)),
        alignment: None,
        child: Size::new(20.0, 20.0),
        constraints: loose(200.0),
        color: None,
        decoration: None,
        transform: None,
        child_overflow_transform: None,
    });

    assert_container_matches_stack(ContainerStackCase {
        case: "tight incoming constraints with a centred child",
        margin: EdgeInsets::ZERO,
        extra: None,
        padding: Some(EdgeInsets::all(12.0)),
        alignment: Some(Alignment::CENTER),
        child: Size::new(40.0, 40.0),
        constraints: BoxConstraints::tight(Size::new(200.0, 200.0)),
        color: None,
        decoration: None,
        transform: None,
        child_overflow_transform: None,
    });

    assert_container_matches_stack(ContainerStackCase {
        case: "a minimum on one axis only",
        margin: EdgeInsets::all(7.0),
        extra: Some(BoxConstraints::new(0.0, unbounded, 150.0, unbounded)),
        padding: Some(EdgeInsets::ZERO),
        alignment: None,
        child: Size::new(30.0, 30.0),
        constraints: loose(300.0),
        color: None,
        decoration: None,
        transform: None,
        child_overflow_transform: None,
    });

    assert_container_matches_stack(ContainerStackCase {
        case: "color, decoration and a translation wrap the same child",
        margin: EdgeInsets::all(5.0),
        extra: Some(BoxConstraints::new(80.0, unbounded, 0.0, unbounded)),
        padding: Some(EdgeInsets::all(8.0)),
        alignment: Some(Alignment::TOP_LEFT),
        child: Size::new(30.0, 20.0),
        constraints: loose(200.0),
        color: Some(Color::RED),
        decoration: Some(BoxDecoration::with_color(Color::BLUE)),
        transform: Some(Matrix4::translation(10.0, 4.0, 0.0)),
        child_overflow_transform: None,
    });

    // Every case above hands the Align level a BOUNDED incoming axis —
    // `extra`'s own unbounded maxes (where present) are clamped back to
    // finite by `additional.enforce(&after_margin)` before they ever reach
    // it. `positioned_box_size`'s shrink-wrap fork
    // (`constraints.max_width.is_infinite()`, `align.rs`) only fires on a
    // genuinely unbounded axis, which none of the above ever supplies — no
    // `extra` here, so the incoming width's own infinity survives margin and
    // padding deflation unclamped.
    //
    // What this specifically pins is REACHABILITY, not an independent check
    // of the fork's behavior: `RenderContainer::perform_layout` calls the
    // exact same free `positioned_box_size` a real `RenderAlign` does, with
    // the exact same derived constraints, so a bug inside that shared
    // function is wrong identically on both sides of the diff below and a
    // value mismatch would never surface it — this case was unreachable
    // before, not undiffed once reached. What DOES make a wrong fork fail
    // here is `debug_assert_layout_output` (`box_protocol.rs`): an
    // unbounded axis that never shrinks commits a non-finite size on
    // `RenderContainer` itself, which panics before either tree's geometry
    // is compared. The absolute pin right after this case is the
    // independent reference: a fixed expected size and child offset, computed
    // by hand from the constraint math, not from the shared function.
    let unbounded_case = ContainerStackCase {
        case: "an unbounded incoming width shrink-wraps to the child under alignment",
        margin: EdgeInsets::all(3.0),
        extra: None,
        padding: Some(EdgeInsets::all(4.0)),
        alignment: Some(Alignment::CENTER),
        child: Size::new(30.0, 20.0),
        constraints: BoxConstraints::new(0.0, unbounded, 0.0, 200.0),
        color: None,
        decoration: None,
        transform: None,
        child_overflow_transform: None,
    };
    assert_container_matches_stack(unbounded_case.clone());

    // Independent of `assert_container_matches_stack`: a fixed expected
    // size and child offset for the exact same configuration, computed by
    // hand — content shrinks to the child's 30 width; height expands to
    // 200 − margin.vertical(6) − padding.vertical(8) = 186; CENTER splits
    // the child's 166px of vertical slack in half. Padding(4) + margin(3) +
    // that 83 gives the child's absolute offset (7, 90); the outer box is
    // margin(3×2) + padding(4×2) + content (30×186) = 44×200.
    let unbounded = RenderTester::mount(
        box_node(
            RenderContainer::new()
                .with_margin(unbounded_case.margin)
                .with_padding(unbounded_case.padding.expect("case sets padding"))
                .with_alignment(unbounded_case.alignment.expect("case sets alignment")),
        )
        .child(
            box_node(RenderColoredBox::new(
                [0.0, 0.0, 1.0, 1.0],
                unbounded_case.child,
            ))
            .label("child"),
        ),
    )
    .with_constraints(unbounded_case.constraints)
    .run_frame();
    assert_eq!(
        unbounded.box_geometry(unbounded.root()),
        Size::new(44.0, 200.0),
        "the unbounded-width case's outer box must be exactly 44×200"
    );
    assert_eq!(
        unbounded.offset(unbounded.id("child")),
        Offset::new(7.0, 90.0),
        "the unbounded-width case's child must land at exactly (7, 90)"
    );

    // A non-zero margin, NOTHING else set, no alignment — `padding: None`
    // means `Container.build` inserts no level at all between
    // `Padding(margin)` and the child (`_paddingIncludingDecoration` is
    // null), so a scaled child whose own `hit_test` does not bound itself
    // (`RenderTransform`, deliberately, so a visually-overflowing child
    // stays hittable across its whole painted area) is reachable straight
    // through the margin band — both trees must AGREE it hits there, not
    // that it doesn't.
    assert_container_matches_stack(ContainerStackCase {
        case: "a non-zero margin with nothing else set does not gate an overflowing scaled child",
        margin: EdgeInsets::all(10.0),
        extra: None,
        padding: None,
        alignment: None,
        child: Size::new(30.0, 30.0),
        constraints: loose(200.0),
        color: None,
        decoration: None,
        transform: None,
        child_overflow_transform: Some(Matrix4::scaling(2.0, 2.0, 1.0)),
    });

    // The mirror case, with exactly one property added: a real `padding`
    // level. Now `Container.build` inserts `Padding(padding)` between
    // `Padding(margin)` and the child, and its own box (`inner_size`) gates
    // the same overflowing child before it is ever reached — so the overflow
    // must NOT leak into the margin band here. This is the "inside the
    // margin band (right)" / "(bottom)" probes' load-bearing case: the
    // scale(2) pivot sits at the child's own top-left, so the overflow grows
    // toward the right/bottom, past the inner box and into the margin there,
    // never toward the left/top.
    assert_container_matches_stack(ContainerStackCase {
        case: "a non-zero margin with a real padding level gates an overflowing scaled child",
        margin: EdgeInsets::all(10.0),
        extra: None,
        padding: Some(EdgeInsets::all(5.0)),
        alignment: None,
        child: Size::new(30.0, 30.0),
        constraints: loose(200.0),
        color: None,
        decoration: None,
        transform: None,
        child_overflow_transform: Some(Matrix4::scaling(2.0, 2.0, 1.0)),
    });
}

/// A childless container stands in for a placeholder subtree,
/// `LimitedBox(0, 0, child: ConstrainedBox(expand))`: it fills the space it is
/// given, and collapses where that space is unbounded.
fn harness_container_childless_fills_bounded_and_collapses_unbounded() {
    let bounded = RenderTester::mount(box_node(RenderContainer::new()))
        .with_constraints(loose(200.0))
        .run_layout();
    assert_eq!(
        bounded.box_geometry(bounded.root()),
        Size::new(200.0, 200.0),
    );

    let unbounded = RenderTester::mount(box_node(RenderContainer::new()))
        .with_constraints(BoxConstraints::new(0.0, f64::INFINITY, 0.0, f64::INFINITY))
        .run_layout();
    assert_eq!(
        unbounded.box_geometry(unbounded.root()),
        Size::ZERO,
        "an unbounded axis must collapse, not expand to infinity"
    );

    // A half-unbounded axis resolves independently of the other.
    let half = RenderTester::mount(box_node(RenderContainer::new()))
        .with_constraints(BoxConstraints::new(0.0, 300.0, 0.0, f64::INFINITY))
        .run_layout();
    assert_eq!(half.box_geometry(half.root()), Size::new(300.0, 0.0),);
}

/// A singular matrix compresses the subtree below a pixel, so nothing is
/// painted and nothing is hit.
fn harness_container_singular_transform_paints_and_hits_nothing() {
    let run = RenderTester::mount(
        box_node(RenderContainer::new().with_transform(Matrix4::scaling(0.0, 1.0, 1.0)))
            .child(box_node(RenderColoredBox::blue(20.0, 20.0)).label("child")),
    )
    .with_size(Size::new(100.0, 100.0))
    .run_frame();

    assert_eq!(run.hit_first(5.0, 5.0), None);
    assert!(
        run.display_commands().is_empty(),
        "a singular transform must skip paint entirely, child included; \
         commands: {:#?}",
        run.display_commands()
    );
}

fn harness_limited_box_caps_unbounded_width_in_row() {
    let run = RenderTester::mount(
        box_node(RenderFlex::row()).child(
            box_node(RenderLimitedBox::width(60.0))
                .child(box_node(RenderColoredBox::green(200.0, 20.0)).label("child")),
        ),
    )
    .with_size(Size::new(200.0, 100.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.id("child")).width, 60.0);
}

fn harness_offstage_hidden_collapses_and_misses_hits() {
    let run = RenderTester::mount(
        box_node(RenderOffstage::hidden())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    // Under LOOSE constraints `constraints.smallest()` is zero, so the box does
    // collapse — but only incidentally. See the two tests below.
    assert_eq!(run.box_geometry(run.root()), Size::ZERO);
    assert_eq!(run.hit(10.0, 10.0), [] as [flui_foundation::RenderId; 0]);
    assert!(
        run.descendant_property("RenderOffstage", "offstage")
            .is_some()
    );
}

/// An offstage subtree is dropped from the semantics walk. The
/// node's own config is still built; only its descendants vanish.
///
/// Red-check: delete `RenderOffstage::excludes_semantics_subtree`; the child's
/// labelled semantics node reappears in the tree.
fn harness_offstage_hidden_drops_its_semantics_subtree() {
    let annotated = || {
        box_node(
            RenderSemanticsAnnotations::new(SemanticsProperties::new().with_label("Hidden"))
                .with_container(true),
        )
        .child(box_node(RenderSizedBox::new(Some(40.0), Some(20.0))))
    };

    let hidden = RenderTester::mount(box_node(RenderOffstage::hidden()).child(annotated()))
        .with_constraints(loose(200.0))
        .with_semantics_enabled()
        .run_to_semantics();

    let owner = hidden.semantics_owner().expect("semantics enabled");
    assert!(
        !owner
            .tree()
            .iter()
            .any(|(_, node)| node.label() == Some("Hidden")),
        "an offstage subtree must not reach the semantics tree"
    );

    // Control: visible, the same child is announced — so the assertion above is
    // not vacuous.
    let visible = RenderTester::mount(box_node(RenderOffstage::visible()).child(annotated()))
        .with_constraints(loose(200.0))
        .with_semantics_enabled()
        .run_to_semantics();

    let owner = visible.semantics_owner().expect("semantics enabled");
    assert!(
        owner
            .tree()
            .iter()
            .any(|(_, node)| node.label() == Some("Hidden")),
        "control: a visible child is announced"
    );
}

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

// ── RenderAnimatedOpacity ────────────────────────────────────────────────

fn ticking_controller(ms: u64, value: f64) -> AnimationController {
    let controller = AnimationController::new(Duration::from_millis(ms), &UpdateScheduler::new());
    controller.set_value(value);
    controller
}

/// Wraps `controller` in a [`ProxyAnimation<f64>`] — the composed-animation
/// shape `RenderAnimatedOpacity::new` now takes. `controller` is an
/// `Arc`-backed shared handle, so the caller's own clone keeps driving the
/// SAME underlying state the proxy wraps (`controller.set_value` after this
/// call is still observed).
fn animation_from(controller: &AnimationController) -> ProxyAnimation<f64> {
    let parent: Arc<dyn Animation<f64>> = Arc::new(controller.clone());
    ProxyAnimation::new(parent)
}

fn harness_animated_opacity_paint_alpha_tracks_controller_value_at_0_partial_255() {
    for (value, expect_layer) in [(0.0, false), (0.5, true), (1.0, false)] {
        let controller = ticking_controller(100, value);
        let run = RenderTester::mount(
            box_node(RenderAnimatedOpacity::new(
                animation_from(&controller),
                false,
            ))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
        )
        .with_constraints(loose(200.0))
        .run_frame();

        assert_eq!(
            run.structure().contains(&"Opacity"),
            expect_layer,
            "opacity={value} must {}emit an OpacityLayer (alpha 0/255 -> \
             no layer): {:?}",
            if expect_layer { "" } else { "NOT " },
            run.structure(),
        );
    }
}

fn harness_semantics_annotations_builds_semantics_node_and_passes_layout() {
    let mut properties = SemanticsProperties::new()
        .with_label("Submit")
        .with_button(true)
        .with_enabled(true);
    properties.toggled = Some(false);

    let run = RenderTester::mount(
        box_node(RenderSemanticsAnnotations::new(properties).with_container(true))
            .label("semantics")
            .child(box_node(RenderSizedBox::new(Some(40.0), Some(20.0)))),
    )
    .with_constraints(loose(200.0))
    .with_semantics_enabled()
    .run_to_semantics();

    assert_eq!(run.box_geometry(run.id("semantics")), Size::new(40.0, 20.0),);
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

fn harness_indexed_semantics_reports_its_index_and_only_republishes_on_change() {
    let run = RenderTester::mount(
        box_node(RenderIndexedSemantics::new(11))
            .label("indexed")
            .child(box_node(RenderSemanticsAnnotations::new(
                SemanticsProperties::new().with_label("Row"),
            ))),
    )
    .with_constraints(loose(200.0))
    .with_semantics_enabled()
    .run_to_semantics();

    let owner = run.semantics_owner().expect("semantics enabled");
    let root_id = owner.root().expect("indexed semantics root");
    let node = owner.get(root_id).expect("root id must resolve");
    assert_eq!(
        node.config().index_in_parent(),
        Some(11),
        "the index reaches the semantics configuration zero-based, as the \
         reference carries it",
    );

    // The setter's early return is not a micro-optimisation: an item's wrapper
    // is rebuilt every time the band moves, so a setter that always marked
    // would republish the subtree's semantics on every frame of a scroll.
    let mut object = RenderIndexedSemantics::new(11);
    assert_eq!(
        object.set_index(11),
        flui_rendering::RenderUpdateImpact::NONE,
        "an unchanged index must not request a semantics update",
    );
    assert_eq!(
        object.set_index(12),
        flui_rendering::RenderUpdateImpact::SEMANTICS,
    );
    assert_eq!(object.index(), 12);
}

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
    assert_eq!(node.children(), []);
    assert!(node.config().is_button());
    let label = node.label().expect("merged label");
    assert!(label.contains("Alpha") && label.contains("Beta"));
}

fn harness_exclude_semantics_drops_descendant_content_but_keeps_layout() {
    let hidden = SemanticsProperties::new().with_label("Hidden");

    let run = RenderTester::mount(
        box_node(RenderExcludeSemantics::default())
            .label("exclude")
            .child(
                box_node(RenderSemanticsAnnotations::new(hidden))
                    .child(box_node(RenderSizedBox::new(Some(24.0), Some(16.0)))),
            ),
    )
    .with_constraints(loose(200.0))
    .with_semantics_enabled()
    .run_to_semantics();

    assert_eq!(run.box_geometry(run.id("exclude")), Size::new(24.0, 16.0),);
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

fn harness_fitted_box_preserves_aspect_ratio_when_sizing_box() {
    // child 100×50 (aspect 2.0); under maxW=60 with loose height, Contain sizes
    // the BOX preserving aspect → (60, 30), not a plain clamp (60, 50). Before
    // the fix perform_layout used a plain constrain → (60, 50), disagreeing with
    // compute_dry_layout.
    let run = RenderTester::mount(
        box_node(RenderFittedBox::new(
            BoxFit::Contain,
            Alignment::CENTER,
            Clip::None,
        ))
        .child(box_node(RenderColoredBox::red(100.0, 50.0)).label("child")),
    )
    .with_constraints(BoxConstraints::new(0.0, 60.0, 0.0, f64::INFINITY))
    .run_layout();
    assert_eq!(run.box_geometry(run.root()), Size::new(60.0, 30.0));
}

fn harness_fractionally_sized_box_applies_width_factor() {
    let run = RenderTester::mount(
        box_node(RenderFractionallySizedBox::new().with_width_factor(FractionFactor::HALF))
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .with_size(Size::new(200.0, 100.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.id("child")).width, 100.0);
}

fn harness_fractional_translation_hits_shifted_child_outside_own_bounds() {
    // translation (1.0, 0.0) shifts the 40×40 child to visual x ∈ [40, 80). A
    // pointer at (50, 20) is OUTSIDE the box's own [0,40) bounds but inside the
    // shifted child → must hit (child-local (10, 20)). The own-bounds check
    // is skipped; the prior `is_within_own_size` gate returned no hit here.
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

fn harness_decorated_box_circle_shape_hit_test_misses_the_corner() {
    // BoxShape::Circle inscribes the circle in the shorter side (here: the
    // full 100x100 square, r=50, centered at (50,50)). (4,4) is inside the
    // 100x100 bounding rect but far outside the inscribed circle
    // (distance from center ~= 65 > 50), so it must MISS -- there is no
    // child to fall back to, so a hit at (4,4) means the decoration's own
    // shape (a plain rect fallback) wrongly claimed the corner.
    let run = RenderTester::mount(box_node(RenderDecoratedBox::new(
        BoxDecoration::with_color(Color::RED).set_shape(BoxShape::Circle),
    )))
    .with_size(Size::new(100.0, 100.0))
    .run_frame();

    assert_eq!(
        run.hit_first(4.0, 4.0),
        None,
        "a BoxShape::Circle decoration must not claim its corners as hits"
    );

    // Paired positive assertion: without this, `hit_test_self` returning
    // `false` unconditionally (never testing the circle at all, just
    // rejecting everything) would also make the corner-miss assertion
    // above pass.
    assert!(
        run.hit_first(50.0, 50.0).is_some(),
        "the center of a BoxShape::Circle decoration must still hit"
    );
}

fn harness_decorated_box_paints_background_before_child() {
    // With the default `DecorationPosition::Background`, the decoration's fill must
    // land on the canvas BEFORE the child's. `run.painted()` (a layer tree
    // exists somewhere) would stay green even if the decoration painted the
    // wrong color or after the child instead of before it. Assert the actual
    // draw commands and their order, mirroring
    // `harness_custom_paint_orders_background_child_foreground`.
    let run = RenderTester::mount(
        box_node(RenderDecoratedBox::new(BoxDecoration::with_color(
            Color::RED,
        )))
        .child(box_node(RenderColoredBox::blue(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    // Layout stays a passthrough to the child even though paint now does
    // real work: the decorated box must not claim any size of its own.
    assert_eq!(run.box_geometry(run.root()), Size::new(40.0, 40.0));

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
        2,
        "expected the decoration's background fill and the child's fill; commands:\n{}",
        painted.join("\n"),
    );
    assert!(
        rects[0].contains("#FF0000FF") && rects[1].contains("#0000FFFF"),
        "paint order must be background red -> child blue; commands:\n{}",
        painted.join("\n"),
    );
}

fn harness_clip_rect_self_describes() {
    let run = RenderTester::mount(
        box_node(RenderClipRect::new(Clip::HardEdge))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(&run.diagnostics(), "RenderClipRect", &["clip_behavior"]);
    assert_eq!(run.box_geometry(run.root()), Size::new(40.0, 40.0));
}

fn harness_clip_rrect_wraps_child() {
    let run = RenderTester::mount(
        box_node(RenderClipRRect::new(Clip::AntiAlias))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(&run.diagnostics(), "RenderClipRRect", &["clip_behavior"]);
    assert_eq!(run.box_geometry(run.root()), Size::new(40.0, 40.0));
}

fn harness_clip_oval_wraps_child() {
    let run = RenderTester::mount(
        box_node(RenderClipOval::new(Clip::AntiAlias))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(&run.diagnostics(), "RenderClipOval", &["clip_behavior"]);
    assert_eq!(run.box_geometry(run.root()), Size::new(40.0, 40.0));
}

fn harness_clip_path_wraps_child() {
    let run = RenderTester::mount(
        box_node(RenderClipPath::new(Clip::AntiAlias))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_descendant_properties(&run.diagnostics(), "RenderClipPath", &["clip_behavior"]);
    assert_eq!(run.box_geometry(run.root()), Size::new(40.0, 40.0));
}

// ============================================================================
// RenderShaderMask / RenderBackdropFilter
// ============================================================================

/// A trivial shader for tests that don't care about the produced shader itself,
/// only that the mask machinery ran.
fn solid_white_shader() -> Shader {
    Shader::solid(Color::WHITE)
}

fn harness_shader_mask_paints_with_shader_mask_layer() {
    let run = RenderTester::mount(
        box_node(RenderShaderMask::new(solid_white_shader()))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert!(run.painted());
    assert!(run.structure().contains(&"ShaderMask"));
}

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

// ============================================================================
// RenderLeaderLayer / RenderFollowerLayer
// ============================================================================

fn harness_leader_layer_always_pushes_layer_even_with_zero_children() {
    // Regression test for the highest-risk trap in the design research
    // plan: unlike ShaderMask/BackdropFilter's OWN no-child
    // test (which asserts the layer is ABSENT), `RenderLeaderLayer::paint`
    // pushes its `LeaderLayer` UNCONDITIONALLY — a childless leader is still
    // a coordinate anchor and must still appear in the structure.
    let run = RenderTester::mount(box_node(RenderLeaderLayer::new(LayerLink::new())))
        .with_constraints(loose(200.0))
        .run_frame();

    assert!(
        run.structure().contains(&"Leader"),
        "a childless Leader MUST still push its layer (unconditional push, \
         unlike ShaderMask/BackdropFilter): {:?}",
        run.structure(),
    );
}

/// a leader+follower pair under two DIFFERENT
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
///              └─ RenderAlign(TOP_LEFT)       (the `Positioned.fill` + `Align`
///                    └─ "follower_child"       idiom for a follower whose
///                       = RenderColoredBox      resolved position can land
///                         (30x30)               anywhere in the overlay)
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
        .with_size(Size::new(300.0, 300.0))
        .run_frame();

    // (a) A hit at the follower's RESOLVED on-screen position — the
    // leader's absolute anchor at `branch_a`'s Stack offset (50,60), well
    // inside the follower_child's resolved (50,60)-(80,90) rect — reaches
    // the follower's child.
    assert_eq!(
        run.hit_first(60.0, 70.0),
        Some(run.id("follower_child")),
        "a hit at the follower's RESOLVED on-screen position must reach \
         its child — this is the whole point of follower hit-testing"
    );

    // (b) A hit at the follower's plain TREE-RELATIVE position (inside
    // `follower_child`'s NATURAL (0,0)-(30,30) rect, where `branch_b` and
    // the follower itself sit) does NOT reach the child — a naive
    // structural-only forward (the older behavior) would have hit
    // it here instead, exactly backwards.
    assert_eq!(
        run.hit_first(10.0, 10.0),
        None,
        "a hit at the follower's plain TREE-RELATIVE position must NOT \
         reach its child — this is the regression proof that the fix is \
         real, not a no-op (a naive structural-only forward hits here)"
    );
}

// ============================================================================
// RenderPhysicalModel / RenderPhysicalShape
// ============================================================================

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

// The `usesSaveLayer` fork (research plan trap) — controls WHERE the
// fill is drawn, not just whether. These two tests are the direct check
// that a naive port didn't collapse the fork into "always fill outside"
// or "always fill inside" (either would double-paint or bleed an edge).
//

fn harness_physical_shape_hit_test_triangular_clipper() {
    let lane = InteractionLane::try_new().expect("interaction lane");
    let handle = lane.dispatch_handle();
    let run = lane.enter(|| {
        let target = handle
            .register_path_clipper(|size: Size| {
                let mut p = Path::new();
                p.move_to(Point::new(size.width * 0.5, 0.0));
                p.line_to(Point::new(size.width, size.height));
                p.line_to(Point::new(0.0, size.height));
                p.close();
                p
            })
            .expect("register triangle path target");
        RenderTester::mount(
            box_node(RenderPhysicalShape::new(Color::WHITE).with_path_clip_target(target))
                .child(box_node(RenderColoredBox::red(100.0, 100.0)).label("child")),
        )
        .with_size(Size::new(100.0, 100.0))
        .run_layout()
    });

    // Gating on a custom clipper and the "always test shape" convention
    // already agree for `RenderPhysicalShape` (it always has a clipper), so
    // this is a plain shape hit-test, not a divergence test.
    assert_eq!(
        lane.enter(|| run.hit_first(1.0, 1.0)),
        None,
        "top-left bounding-box corner is outside the triangle"
    );
    assert_eq!(
        lane.enter(|| run.hit_first(50.0, 90.0)),
        Some(run.id("child")),
        "near the base midpoint must be inside the triangle"
    );
}

fn harness_repaint_boundary_splits_layer_tree() {
    let run = RenderTester::mount(
        box_node(RenderRepaintBoundary::new())
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(run.structure(), vec!["Offset", "Picture"]);
}

fn harness_metadata_with_payload() {
    let run = RenderTester::mount(
        box_node(RenderMetaData::new().with_metadata(42u32))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(run.box_geometry(run.root()), Size::new(40.0, 40.0));
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderMetaData",
        &["has_metadata", "behavior"],
    );
}

// ============================================================================
// Multi-child box objects
// ============================================================================

fn harness_flex_row_positions_children_on_main_axis() {
    let run = RenderTester::mount(
        box_node(RenderFlex::row())
            .child(box_node(RenderColoredBox::red(30.0, 20.0)).label("a"))
            .child(box_node(RenderColoredBox::green(50.0, 20.0)).label("b")),
    )
    .with_size(Size::new(200.0, 100.0))
    .run_layout();

    assert_eq!(run.offset(run.id("a")), Offset::ZERO);
    assert_eq!(run.offset(run.id("b")), Offset::new(30.0, 0.0));
    assert_eq!(
        run.descendant_property("RenderFlex", "direction")
            .as_deref(),
        Some("Horizontal"),
    );
}

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
    .with_size(Size::new(100.0, 100.0))
    .run_frame();

    assert_eq!(run.offset(run.id("badge")), Offset::new(16.0, 8.0));
    assert_eq!(run.hit_first(20.0, 12.0), Some(run.id("badge")));
    assert_eq!(run.hit_first(5.0, 5.0), Some(run.id("base")));
}

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
        Size::new(80.0, 60.0),
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

fn harness_list_body_vertical_down_stretches_cross_axis_and_hits_children() {
    let constraints = BoxConstraints::new(0.0, 100.0, 0.0, f64::INFINITY);
    let run = RenderTester::mount(
        box_node(RenderListBody::new())
            .child(box_node(RenderSizedBox::fixed(20.0, 10.0)).label("first"))
            .child(box_node(RenderSizedBox::fixed(30.0, 20.0)).label("second")),
    )
    .with_constraints(constraints)
    .run_frame();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(100.0, 30.0),
        "vertical ListBody must take the bounded cross-axis width and summed child heights",
    );
    assert_eq!(
        run.box_geometry(run.id("first")),
        Size::new(100.0, 10.0),
        "children are tight to the cross-axis width",
    );
    assert_eq!(run.offset(run.id("first")), Offset::ZERO);
    assert_eq!(run.offset(run.id("second")), Offset::new(0.0, 10.0));
    assert_eq!(run.hit_first(5.0, 15.0), Some(run.id("second")));
    assert_descendant_properties(&run.diagnostics(), "RenderListBody", &["axis_direction"]);
}

// ── RenderStack dry layout ────────────────────────────────────────────────────

// ============================================================================
// Pointer semantics
// ============================================================================

/// A hidden `RenderVisibility` keeps its child's geometry — that is the whole
/// contract `Visibility::maintain_size` rests on, so it is asserted as
/// geometry rather than inferred from the flag.
fn harness_visibility_keeps_child_geometry_while_hidden() {
    for visible in [true, false] {
        let run = RenderTester::mount(
            box_node(RenderVisibility::new(visible))
                .child(box_node(RenderColoredBox::red(40.0, 24.0)).label("child")),
        )
        .with_constraints(loose(200.0))
        .run_layout();

        assert_eq!(
            run.box_geometry(run.root()),
            Size::new(40.0, 24.0),
            "visible={visible}: the child's size must survive being hidden"
        );
        assert_eq!(
            run.box_geometry(run.id("child")),
            Size::new(40.0, 24.0),
            "visible={visible}: the child itself is still laid out"
        );
    }
}

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
    .with_size(Size::new(100.0, 100.0))
    .run_frame();

    let path = run.hit(20.0, 20.0);
    assert!(path.contains(&run.id("absorb")));
    assert!(!path.contains(&run.id("inner")));
}

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
    .with_size(Size::new(100.0, 100.0))
    .run_frame();

    assert_eq!(run.hit_first(20.0, 20.0), Some(run.id("below")));
}

// ============================================================================
// Sliver objects (via viewport host)
// ============================================================================

fn harness_sliver_fixed_extent_list_geometry() {
    let run = RenderTester::mount(viewport(
        fixed_extent_list(
            25.0,
            vec![
                box_node(RenderColoredBox::red(300.0, 1000.0)).label("item0"),
                box_node(RenderColoredBox::green(300.0, 1000.0)),
            ],
        )
        .label("list"),
    ))
    .with_size(Size::new(300.0, 100.0))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("list")).scroll_extent, 50.0);
    assert_eq!(run.box_geometry(run.id("item0")).height, 25.0);
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderSliverFixedExtentList",
        &["item_extent"],
    );
    let tree = run.diagnostics();
    let sliver = tree.find_descendant("RenderSliverFixedExtentList").unwrap();
    assert_has_committed_geometry(sliver);
}

// The index-helper cases (max child index for a scroll offset, item-extent
// references and the rounding-error layout) are unit tests on the index
// helpers inside
// `crates/flui-objects/src/sliver/sliver_fixed_extent_list.rs` (the tolerance
// nudges are `f64`-scaled there, see that module's mapping decisions).
// Child-manager residency is covered by the pins above through the request
// and retain-band sinks.
//
// Non-finite scroll-window edges (`NaN` / `±∞`) cannot be injected through a
// healthy viewport host; the finite-domain contract and empty-band fallback
// are proven on `window` / the index helpers in that same unit-test module.

// ── RenderSliverGrid ─────────────────────────────────────────────────────────

// The two former stale-out-of-band `hit_test`/`paint` pins
// (`hit_test_ignores_stale_out_of_band_offset`, `paint_ignores_stale_out_of_band_tile`)
// pinned an object-level `laid_out_band` gate that only the now-deleted eager
// grid carried — this render object keeps no such band, and cannot: `PaintCx`
// exposes neither parent data nor a child's logical index. The hazard is
// closed at the PIPELINE instead, and structurally rather than by discipline:
// a layout stamps its own generation onto the children it laid out, and the
// paint driver skips any child carrying a different one. Every multi-child
// object gets the property for free, not just the ones that remembered to
// track a band. No test pins the paint-side skip;
// `harness_placed_generation_gate_excludes_a_dropped_child_from_semantics`
// pins the same stamp on the semantics walk.

/// Regression for a NARROWER, review-caught defect ported from the eager
/// grid's original fix: what a poisoned relayout must not lose is the
/// pipeline's committed child offsets and this object's own resident count —
/// neither rolls back when `perform_layout` is interrupted, both freeze at
/// whatever the LAST SUCCESSFUL pass left. The pipeline wraps every render
/// object's `perform_layout` call in `catch_unwind`
/// (`crates/flui-rendering/src/pipeline/owner/subtree_arena.rs`); if the
/// caller-supplied `grid_delegate` panics partway through a pass, an
/// interrupted commit would leave stale state pointing at a window the
/// aborted pass never finished writing.
///
/// `SliverGridLayout::compute_max_scroll_offset` divides by
/// `cross_axis_count` (`(child_count - 1) / self.cross_axis_count`) with no
/// zero guard — an unconditional (not debug-only) integer division-by-zero
/// panic for any delegate whose `get_layout` returns `cross_axis_count: 0`.
/// `SliverGridDelegate` is a public trait implemented by caller code, and
/// nothing in its contract (or `SliverGridLayout`'s public fields) rules out
/// a degenerate return value, so this is a real, externally reachable panic
/// site — not a contrived one. `RenderSliverGrid::perform_layout` calls it
/// while computing `scroll_extent`, AFTER the layout loop but BEFORE it
/// commits `attached_child_count`, so the panic leaves both the resident
/// count and the committed offsets exactly as the last successful pass left
/// them.
fn harness_render_sliver_grid_hit_test_keeps_pre_panic_band_after_a_poisoned_relayout() {
    #[derive(Debug)]
    struct ZeroCrossAxisCountDelegate;

    impl SliverGridDelegate for ZeroCrossAxisCountDelegate {
        fn get_layout(&self, _constraints: SliverConstraints) -> SliverGridLayout {
            SliverGridLayout {
                cross_axis_count: 0,
                main_axis_stride: 100.0,
                cross_axis_stride: 100.0,
                child_main_axis_extent: 100.0,
                child_cross_axis_extent: 100.0,
                reverse_cross_axis: false,
            }
        }

        fn should_relayout(&self, _old_delegate: &dyn SliverGridDelegate) -> bool {
            true
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    // Pass 1: an ordinary 2-column delegate over 4 pre-seeded (resident)
    // tiles lays out and positions all of them for real — the "last
    // successful pass" this test's assertions must survive back to.
    let mut run = RenderTester::mount(viewport(
        grid_list(
            Arc::new(SliverGridDelegateWithFixedCrossAxisCount::new(2)),
            vec![
                box_node(RenderColoredBox::red(100.0, 100.0)).label("tile0"),
                box_node(RenderColoredBox::green(100.0, 100.0)).label("tile1"),
                box_node(RenderColoredBox::blue(100.0, 100.0)).label("tile2"),
                box_node(RenderColoredBox::red(100.0, 100.0)).label("tile3"),
            ],
        )
        .label("grid"),
    ))
    .with_size(Size::new(200.0, 200.0))
    .run_layout();

    let grid_id = run.id("grid");
    let tile0 = run.id("tile0");
    assert_eq!(
        run.offset(tile0),
        Offset::new(0.0, 0.0),
        "pass 1 must position tile0 normally before the poisoned pass 2",
    );
    let count_before = run.descendant_property("RenderSliverGrid", "attached_child_count");
    assert_eq!(
        count_before.as_deref(),
        Some("4"),
        "pass 1 must commit all 4 residents before the poisoned pass 2; got {count_before:?}",
    );

    // Pass 2: swap in the poisoned delegate and relayout directly (not via
    // `LayoutRun::relayout`, which `.expect()`s success on the OVERALL frame
    // — orthogonal to this test, since the pipeline's own per-node
    // resilience catches this panic and keeps the frame `Ok`; only THIS
    // render object's own state is at risk, which is what the rest of this
    // test verifies directly).
    run.update::<RenderSliverGrid>(grid_id, |grid| {
        let impact = grid.set_grid_delegate(Arc::new(ZeroCrossAxisCountDelegate));
        assert_eq!(impact, flui_rendering::RenderUpdateImpact::LAYOUT);
    });
    let result = run.owner_mut().run_layout();
    assert!(
        result.is_ok(),
        "the pipeline's per-node catch_unwind must keep the overall pass Ok even \
         when one render object's delegate panics; got {result:?}",
    );
    let count_after = run.descendant_property("RenderSliverGrid", "attached_child_count");
    assert_eq!(
        count_after, count_before,
        "the poisoned pass 2 (which panics computing scroll_extent before its own \
         commit statement runs) must leave `attached_child_count` exactly as pass 1 \
         committed it",
    );

    // tile0's offset must be UNCHANGED from pass 1: the panic fires before
    // pass 2's position loop runs, so pass 2 never called `ctx.position_child`
    // at all.
    assert_eq!(
        run.offset(tile0),
        Offset::new(0.0, 0.0),
        "pass 2 panicked before its position loop ran; tile0's committed \
         offset must still be pass 1's",
    );

    // The actual regression: hit_test must keep resolving against pass 1's
    // committed state, not a state the poisoned pass never finished writing.
    assert_eq!(
        run.hit_first(50.0, 50.0),
        Some(tile0),
        "hit_test must keep resolving against the LAST SUCCESSFUL pass's \
         state after a poisoned relayout",
    );
}

// Not covered here: a grid scroll scenario that constructs 60 pre-existing
// `RenderBox` children via a fake child manager, scrolls the SAME mounted
// `RenderViewport` through four offsets, and asserts which of the 60 boxes are
// attached at each position — i.e. a genuine child-manager
// request/attach/evict protocol across multiple relayouts of one persistent
// tree. The reason is a TEST-HARNESS gap, not a production one. This
// harness's `LayoutRun::update`/`relayout` (`crates/flui-rendering/src/testing/
// harness.rs`) DOES support mutating and re-laying-out an already-mounted
// tree — the mechanism itself is not the blocker. The blocker is
// `RenderSliverGrid::perform_layout` (`crates/flui-objects/src/sliver/
// sliver_grid.rs`): resident (already-attached, pre-seeded) slots inside
// the current window get laid out, and absent slots get a
// `ctx.request_child_build` — but nothing detaches slots that fall OUTSIDE
// the window (`ctx.emit_retain_band` only signals the ELEMENT tree's
// `SparseChildren::retain_band` to evict them later — the render level has
// no eviction call of its own; only the element tree can retire a slot,
// which is what prevents an ABA double-remove between the two sides). Since
// this harness mounts render objects directly with no element tree at all,
// there is no consumer for that retain-band signal — pre-seeding all 60
// children and scrolling would show every one still `.attached` forever,
// the OPPOSITE of what such a scenario asserts. Reproducing it needs a fake
// render-level child-manager that creates/disposes real `RenderBox`
// children on demand; this harness's
// `viewport()`/`sliver_node()` builders have no such concept for ANY lazy
// sliver, grid or list alike — a test-infrastructure gap, not something
// wrong in `RenderSliverGrid`'s own (correct, checked separately by the two
// hit-test cases above) behavior.

fn harness_sliver_padding_insets_geometry() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverPadding::symmetric(10.0, 0.0))
            .label("pad")
            .child(fixed_extent_list(
                20.0,
                vec![box_node(RenderColoredBox::red(300.0, 1000.0))],
            )),
    ))
    .with_size(Size::new(300.0, 100.0))
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

fn harness_sliver_to_box_adapter_scroll_extent_matches_child() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverToBoxAdapter::new())
            .label("adapter")
            .child(box_node(RenderSizedBox::fixed(300.0, 42.0)).label("box")),
    ))
    .with_size(Size::new(300.0, 100.0))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("adapter")).scroll_extent, 42.0);
    let tree = run.diagnostics();
    assert_has_committed_geometry(
        tree.find_descendant("RenderSliverToBoxAdapter")
            .expect("adapter"),
    );
}

fn harness_sliver_fill_viewport_fraction() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverFillViewport::new(0.5))
            .label("fill")
            .child(box_node(RenderColoredBox::red(300.0, 1000.0)).label("page")),
    ))
    .with_size(Size::new(300.0, 100.0))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("fill")).scroll_extent, 50.0);
    assert_eq!(run.box_geometry(run.id("page")).height, 50.0);
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderSliverFillViewport",
        &["viewport_fraction"],
    );
}

fn harness_sliver_fill_remaining_uses_viewport_remainder() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverFillRemaining::new())
            .label("fill")
            .child(box_node(RenderColoredBox::red(300.0, 10.0)).label("child")),
    ))
    .with_size(Size::new(300.0, 100.0))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("fill")).scroll_extent, 100.0);
    let tree = run.diagnostics();
    let node = tree.find_descendant("RenderSliverFillRemaining").unwrap();
    assert_has_committed_geometry(node);
}

fn harness_sliver_fill_remaining_and_overscroll_fills_viewport() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverFillRemainingAndOverscroll::new())
            .label("fill")
            .child(box_node(RenderColoredBox::red(300.0, 10.0)).label("child")),
    ))
    .with_size(Size::new(300.0, 100.0))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("fill")).scroll_extent, 100.0);
    assert_eq!(run.box_geometry(run.id("child")).height, 100.0);
    let tree = run.diagnostics();
    let node = tree
        .find_descendant("RenderSliverFillRemainingAndOverscroll")
        .unwrap();
    assert_has_committed_geometry(node);
}

fn harness_sliver_fill_remaining_with_scrollable_reports_full_scroll_extent() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverFillRemainingWithScrollable::new())
            .label("fill")
            .child(box_node(RenderColoredBox::red(300.0, 10.0)).label("child")),
    ))
    .with_size(Size::new(300.0, 100.0))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("fill")).scroll_extent, 100.0);
    assert_eq!(run.box_geometry(run.id("child")).height, 100.0);
    let tree = run.diagnostics();
    let node = tree
        .find_descendant("RenderSliverFillRemainingWithScrollable")
        .unwrap();
    assert_has_committed_geometry(node);
}

fn harness_sliver_ignore_pointer_blocks_hits_when_active() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverIgnorePointer::new(true))
            .label("ignore")
            .child(fixed_extent_list(
                30.0,
                vec![box_node(RenderColoredBox::red(300.0, 1000.0)).label("item")],
            )),
    ))
    .with_size(Size::new(300.0, 100.0))
    .run_frame();

    assert_eq!(run.hit(20.0, 20.0), [] as [flui_foundation::RenderId; 0]);
    assert!(
        run.descendant_property("RenderSliverIgnorePointer", "ignoring")
            .is_some()
    );
}

// ─── RenderSliverList (request seam — INERT without a child manager) ─────────

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
    .with_size(Size::new(300.0, 400.0))
    .run_layout();

    // Items 0 and 1 are in the tree and laid out: offsets must reflect their
    // virtualizer-assigned layout offsets (0 and 48 px) minus scroll_offset=0.
    assert_eq!(
        run.offset(run.id("item0")).dy,
        0.0,
        "resident at logical index 0 must be positioned at dy=0"
    );
    assert_eq!(
        run.offset(run.id("item1")).dy,
        48.0,
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

fn harness_sliver_list_anchor_correction_emits_in_both_scroll_directions() {
    // Two-pass test for the anchor-correction state machine.
    //
    // Setup: 10-item list (48 px seed estimate), item 0 pre-seeded at 60 px.
    // With scroll=100 the viewport tight-visible range starts at item 2
    // (estimated start 96 px < 100 < 144 px = its end) → anchor=(2, 0).
    // Item 0 is in the cache-above band (cache_before = 100 px, cache
    // starts at 0).  set_measured(0, 60, (2,0)) accumulates pending=12.
    // The band's only measured child is 60 px, so the hint for the nine
    // unmeasured items adapts 48 → 60; item 1 sits above the anchor, so the
    // anchor's offset moves 108 → 120 and that +12 joins the accumulator:
    // pending=24.  Forward scroll (last=0 → current=100) → correction EMITTED.
    //
    // The viewport absorbs the correction in its correction loop:
    //   Pass 1 (scroll=100): correction=24 fires → correct_by(24) → pixels=124.
    //   Pass 2 (scroll=124): no new correction; total_extent is now
    //     60 + 9 × 60 = 600, max_scroll = 600 − 400 = 200 ≥ 124, so
    //     apply_content_dimensions accepts; last_scroll_offset finalised to 124.
    // Observable: item 0's paint dy = layout_offset(0) − scroll(124) = −124 px.
    //
    // Pass 2 of this test: grow item 0 to 84 px, scroll BACKWARD to 72 px.
    // Virtualizer item 0 is now Measured at 60 px.  With scroll=72 and the
    // 60 px hint, visible range starts at item 1 (item 0 ends at 60 < 72) →
    // anchor=(1,0).  set_measured(0, 84, (1,0)) accumulates pending=24; the
    // hint adapts 60 → 84 but nothing unmeasured sits above the anchor, so no
    // further correction.  The correction is emitted regardless of scroll
    // direction (ADR-0051 — the old backward suppression was itself a
    // one-frame jump): correct_by(24) → pixels=96, accepted (max_scroll =
    // 840 − 400 = 440).  Item 1 (the anchor, at 84) keeps its screen position
    // 84 − 96 = −12 = 60 − 72; item 0 paint dy = 0 − 96 = −96 px.
    //
    // Fails when anchor-correction is not wired, when a backward scroll
    // withholds the correction (the anchor would drift by the 24 px growth),
    // when the adaptive hint stops feeding the accumulator, or when the
    // viewport's correction loop is broken.
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
    .with_size(Size::new(300.0, 400.0))
    .run_layout();

    let item0_id = run.id("item0");
    let vp_id = run.id("viewport");

    // Pass 1 check: the 24 px forward correction (12 px remeasure + 12 px
    // adaptive re-hint of item 1) was absorbed by the viewport: scroll
    // 100→124 (correct_by), accepted (max_scroll = 600 − 400 = 200).
    // Item 0 at layout_offset=0 with final scroll=124 gets paint dy = -124 px.
    assert_eq!(
        run.offset(item0_id).dy,
        -124.0,
        "forward correction loop: scroll 100→124 (remeasure + adaptive hint); \
         item 0 (layout_offset=0) must have dy=0-124=-124; got {:?}",
        run.offset(item0_id).dy,
    );

    // Pass 2: grow item 0 to 84 px, scroll backward to 72 px.
    run.update::<RenderColoredBox>(item0_id, |b| {
        assert_eq!(
            b.set_preferred_size(Size::new(300.0, 84.0)),
            flui_rendering::RenderUpdateImpact::LAYOUT
        );
    });
    run.update::<RenderViewport<ScrollableViewportOffset>>(vp_id, |vp| {
        vp.offset_mut().set_pixels(72.0);
    });
    run.relayout();

    // Pass 2 check: the 24 px correction is emitted on the backward scroll
    // too → scroll 72→96.  Item 0 at layout_offset=0 gets paint dy = -96 px,
    // and the anchor (item 1) is exactly where it was on screen before the
    // remeasure.
    assert_eq!(
        run.offset(item0_id).dy,
        -96.0,
        "direction-independent correction: scroll 72→96; \
         item 0 (layout_offset=0) must have dy=0-96=-96; got {:?}",
        run.offset(item0_id).dy,
    );
}

fn harness_sliver_offstage_hidden_reports_zero_geometry() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverOffstage::hidden())
            .label("off")
            .child(fixed_extent_list(
                30.0,
                vec![box_node(RenderColoredBox::red(300.0, 1000.0))],
            )),
    ))
    .with_size(Size::new(300.0, 100.0))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("off")).scroll_extent, 0.0);
    assert!(
        run.descendant_property("RenderSliverOffstage", "offstage")
            .is_some()
    );
}

// An alpha=0 sliver must not emit an Opacity layer: alpha 0 → no layer
// painted. The defect this pins: reporting `Some(0)` through `paint_effects().opacity`
// makes the owner wrap the child in a 0-alpha OpacityLayer (present in
// structure); the correct answer at alpha=0 is `None`, no layer emitted.
fn harness_sliver_opacity_alpha_zero_emits_no_opacity_layer() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverOpacity::transparent()) // alpha = 0
            .label("opacity")
            .child(fixed_extent_list(
                30.0,
                vec![box_node(RenderColoredBox::red(300.0, 1000.0))],
            )),
    ))
    .with_size(Size::new(300.0, 100.0))
    .run_frame();

    assert!(
        !run.structure().contains(&"Opacity"),
        "fully-transparent sliver (alpha=0) must NOT emit an OpacityLayer \
         (alpha=0 → no layer): {:?}",
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

// ── RenderSliverAnimatedOpacity ──────────────────────────────────────────

fn harness_sliver_animated_opacity_paint_alpha_tracks_controller_value_at_0_partial_255() {
    for (value, expect_layer) in [(0.0, false), (0.5, true), (1.0, false)] {
        let controller = ticking_controller(100, value);
        let run = RenderTester::mount(viewport(
            sliver_node(RenderSliverAnimatedOpacity::new(
                animation_from(&controller),
                false,
            ))
            .label("opacity")
            .child(fixed_extent_list(
                30.0,
                vec![box_node(RenderColoredBox::red(300.0, 1000.0))],
            )),
        ))
        .with_size(Size::new(300.0, 100.0))
        .run_frame();

        assert_eq!(
            run.structure().contains(&"Opacity"),
            expect_layer,
            "opacity={value} must {}emit an OpacityLayer: {:?}",
            if expect_layer { "" } else { "NOT " },
            run.structure(),
        );
    }
}

fn harness_viewport_stacks_two_slivers() {
    let run = RenderTester::mount(viewport_multi([
        fixed_extent_list(20.0, vec![box_node(RenderColoredBox::red(300.0, 1000.0))])
            .label("header"),
        sliver_node(RenderSliverFillRemaining::new())
            .label("body")
            .child(box_node(RenderColoredBox::green(300.0, 10.0)).label("fill_child")),
    ]))
    .with_size(Size::new(300.0, 100.0))
    .run_layout();

    assert_eq!(run.sliver_geometry(run.id("header")).scroll_extent, 20.0);
    assert_eq!(run.sliver_geometry(run.id("body")).scroll_extent, 80.0);
}

// Regression coverage for the `RenderViewport::attempt_layout` sign bug: the
// forward sequence's `overlap` used `center_offset.min(0.0)`
// (== `(-corrected_offset).min(0.0)`) instead of `corrected_offset.min(0.0)`
// when there is no leading reverse-growth group (and `0.0` when there is
// one). At a positive scroll offset with no leading reverse-growth group,
// `overlap` must be `0.0`; with one, it must be `0.0` for BOTH sequences.
// `RenderSliverFillRemainingWithScrollable` reads `constraints.overlap.min(0.0)`
// directly into its `extent` formula, so a wrong sign inflates `extent` and
// silently un-clamps `paint_extent` — the exact failure mode this guards.

// Under FLUI's old `center_sliver_index`, `Some(1)` split the two children
// [0,1) forward / [1,2) reverse — child 0 ("forward_filler") was forward,
// child 1 ("fill") was reverse, and the forward group's own absolute scroll
// offset (`corrected_offset.max(0.0)`) fed straight into "fill"'s
// `sliver_scroll_offset`. Under the current model `center` is the first
// FORWARD child, so `center: Some(1)` now makes child 0 the REVERSE group
// and child 1 ("fill") the FORWARD group instead — and a forward group's own
// `sliver_scroll_offset` is `(-center_offset).max(0.0)`, which is `0.0`
// whenever `center_offset >= 0.0`. With only "fill" in the forward group
// (whose `scroll_extent` is pinned to `viewport_main_axis_extent`), the
// accepted-offset upper bound is exactly `main_axis_extent * anchor`, the
// same point where `center_offset` reaches `0.0` — so THIS sliver, alone,
// can never see a nonzero `sliver_scroll_offset` at a settled offset, and a
// wrongly-negative `overlap` becomes numerically invisible (`to = extent`
// saturates against `b` either way; a first attempt with this shape
// confirmed 100.0 both before and after deliberately reintroducing the sign
// bug). A second forward sibling AFTER "fill" (contributing its own
// `scroll_extent` to `max_scroll_extent`) raises that bound past
// `main_axis_extent * anchor`, so a legitimate offset can push
// `center_offset` negative and give "fill" — still the FIRST forward
// child, still the one `overlap` is computed for — a real nonzero
// `sliver_scroll_offset`.
//

fn harness_shrink_wrapping_viewport_sizes_to_sliver_extent_under_unbounded_main_axis() {
    let run = RenderTester::mount(shrink_wrapping_viewport(
        fixed_extent_list(
            25.0,
            vec![
                box_node(RenderColoredBox::red(300.0, 1000.0)).label("item0"),
                box_node(RenderColoredBox::green(300.0, 1000.0)).label("item1"),
            ],
        )
        .label("list"),
    ))
    .with_constraints(BoxConstraints::new(300.0, 300.0, 0.0, f64::INFINITY))
    .run_layout();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(300.0, 50.0),
        "shrink-wrapping viewport must take its main-axis size from child max_paint_extent"
    );
    assert_eq!(run.sliver_geometry(run.id("list")).scroll_extent, 50.0);
    assert_descendant_properties(
        &run.diagnostics(),
        "RenderShrinkWrappingViewport",
        &["axis_direction", "scroll_offset", "shrink_wrap_extent"],
    );
}

/// A shrink-wrapping viewport on a reverse axis positions its children from
/// the far edge of its FINAL size: two 25 px items under an unbounded main
/// axis make it 50 px tall, and on `BottomToTop` item 0 sits at y = 50 − 25
/// and item 1 at y = 0. The offsets are resolved once, from the accepted
/// pass, against that size — not from a provisional size the pass laid out
/// under.
/// A sliver that lays out cleanly `healthy_layouts` times and panics on
/// every layout after that, so a test can watch what a viewport does when a
/// descendant's layout fails mid-frame. The pipeline catches the panic in
/// the child's own walk frame and hands the parent `SliverGeometry::ZERO`,
/// which is exactly the stand-in a viewport must not publish scroll
/// dimensions from.
#[derive(Debug)]
struct PanicAfterNLayouts {
    extent: f64,
    layouts: Arc<std::sync::atomic::AtomicUsize>,
    healthy_layouts: usize,
}

impl flui_rendering::traits::RenderSliver for PanicAfterNLayouts {
    type Arity = flui_foundation::Leaf;
    type ParentData = flui_rendering::parent_data::SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::SliverLayoutContext<
            '_,
            flui_foundation::Leaf,
            Self::ParentData,
        >,
    ) -> flui_rendering::constraints::SliverGeometry {
        let nth = self
            .layouts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        assert!(
            nth <= self.healthy_layouts,
            "deliberate test failure on layout {nth}"
        );
        let constraints = *ctx.constraints();
        let paint_extent = self.calculate_paint_offset(&constraints, 0.0, self.extent);
        flui_rendering::constraints::SliverGeometry {
            scroll_extent: self.extent,
            paint_extent,
            layout_extent: paint_extent,
            max_paint_extent: self.extent,
            cache_extent: self.calculate_cache_offset(&constraints, 0.0, self.extent),
            hit_test_extent: paint_extent,
            visible: paint_extent > 0.0,
            ..flui_rendering::constraints::SliverGeometry::ZERO
        }
    }
}

impl flui_foundation::Diagnosticable for PanicAfterNLayouts {}

/// The box twin of [`PanicAfterNLayouts`], for the case that matters most:
/// the failure is two levels below the viewport (under a
/// `RenderSliverToBoxAdapter`), where a flag that only reports a direct
/// child's failure sees nothing.
#[derive(Debug)]
struct PanicAfterNBoxLayouts {
    size: Size,
    layouts: Arc<std::sync::atomic::AtomicUsize>,
    healthy_layouts: usize,
}

impl RenderBox for PanicAfterNBoxLayouts {
    type Arity = flui_foundation::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<
            '_,
            flui_foundation::Leaf,
            Self::ParentData,
        >,
    ) -> Size {
        let nth = self
            .layouts
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        assert!(
            nth <= self.healthy_layouts,
            "deliberate test failure on box layout {nth}"
        );
        ctx.constraints().constrain(self.size)
    }
}

impl flui_foundation::Diagnosticable for PanicAfterNBoxLayouts {}

fn harness_viewport_degraded_pass_does_not_move_the_scroll_position() {
    use flui_rendering::view::{ScrollPosition, ViewportOffset};

    let position = ScrollPosition::new(500.0);
    let layouts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let node = box_node(RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        position.clone(),
    ))
    .label("viewport")
    .child(
        fixed_extent_list(
            400.0,
            vec![box_node(RenderColoredBox::red(300.0, 1000.0)).label("head")],
        )
        .label("first"),
    )
    .child(
        sliver_node(PanicAfterNLayouts {
            extent: 400.0,
            layouts: Arc::clone(&layouts),
            healthy_layouts: 1,
        })
        .label("fragile"),
    )
    .child(
        fixed_extent_list(
            400.0,
            vec![box_node(RenderColoredBox::green(300.0, 1000.0)).label("tail")],
        )
        .label("third"),
    );
    let mut run = RenderTester::mount(node)
        .with_size(Size::new(300.0, 400.0))
        .run_layout();
    assert_eq!(
        position.max_scroll_extent(),
        800.0,
        "the healthy pass publishes 1 200 px of content in a 400 px viewport"
    );
    assert_eq!(position.pixels(), 500.0);

    // Pass 2: the middle sliver panics. Both it and the viewport are marked
    // so the walk reaches the viewport's own `perform_layout` with a child
    // that must really lay out (a clean child would serve cached geometry).
    let fragile = run.id("fragile");
    let viewport = run.id("viewport");
    run.owner_mut().mark_needs_layout(fragile);
    run.owner_mut().mark_needs_layout(viewport);
    let _ = run.owner_mut().run_layout();
    assert!(
        layouts.load(std::sync::atomic::Ordering::SeqCst) >= 2,
        "the fragile sliver was laid out again and panicked"
    );
    assert_eq!(
        position.pixels(),
        500.0,
        "a degraded pass must not clamp the user's offset to the stand-in's \
         shorter content"
    );
    assert_eq!(
        position.max_scroll_extent(),
        800.0,
        "and must not publish the stand-in's content extent"
    );

    // Pass 3: the child is poisoned now, so the walk serves its stand-in
    // without recording a failure — the pass is still degraded.
    run.owner_mut().mark_needs_layout(viewport);
    let _ = run.owner_mut().run_layout();
    assert_eq!(
        position.pixels(),
        500.0,
        "the frame after the failure still reads a stand-in and still \
         publishes nothing"
    );
}

/// A viewport skips laying out a child that is entirely beyond its window and
/// serves that child's cached geometry instead. When the cache was built by a
/// degraded pass, serving it would hide the breakage: the broken descendant is
/// never walked, the pass looks healthy, and the collapsed content extent is
/// published.
///
/// Four 400 px children (1 600 px of content) in a 400 px viewport: the third
/// is an adapter over a box that panics after its first layout. The window
/// plus its 250 px cache reaches 650 px, so that adapter — at 800..1200 — is
/// beyond it and is exactly the child the cache path serves.
fn harness_viewport_does_not_serve_a_cache_built_by_a_degraded_pass() {
    use flui_rendering::view::ScrollPosition;

    let position = ScrollPosition::new(0.0);
    let layouts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let node = box_node(RenderViewport::with_offset(
        AxisDirection::TopToBottom,
        AxisDirection::LeftToRight,
        position.clone(),
    ))
    .label("viewport")
    .child(
        fixed_extent_list(
            400.0,
            vec![box_node(RenderColoredBox::red(300.0, 1000.0)).label("head")],
        )
        .label("first"),
    )
    .child(
        fixed_extent_list(
            400.0,
            vec![box_node(RenderColoredBox::blue(300.0, 1000.0)).label("second_cell")],
        )
        .label("second"),
    )
    .child(
        sliver_node(RenderSliverToBoxAdapter::new())
            .label("far_adapter")
            .child(
                box_node(PanicAfterNBoxLayouts {
                    size: Size::new(300.0, 400.0),
                    layouts: Arc::clone(&layouts),
                    healthy_layouts: 1,
                })
                .label("far_box"),
            ),
    )
    .child(
        fixed_extent_list(
            400.0,
            vec![box_node(RenderColoredBox::green(300.0, 1000.0)).label("tail")],
        )
        .label("fourth"),
    );
    let mut run = RenderTester::mount(node)
        .with_size(Size::new(300.0, 400.0))
        .run_layout();
    assert_eq!(
        position.max_scroll_extent(),
        1200.0,
        "the healthy mount publishes 1 600 px of content in a 400 px viewport"
    );

    // Pass 2: the far box panics. Its adapter returns a collapsed geometry
    // and is marked as having committed it in a degraded pass.
    let far_box = run.id("far_box");
    let viewport = run.id("viewport");
    run.owner_mut().mark_needs_layout(far_box);
    run.owner_mut().mark_needs_layout(viewport);
    let _ = run.owner_mut().run_layout();
    assert!(layouts.load(std::sync::atomic::Ordering::SeqCst) >= 2);
    assert_eq!(
        position.max_scroll_extent(),
        1200.0,
        "a degraded pass publishes nothing"
    );

    // Pass 3: only the viewport is marked. The adapter is clean, its
    // constraints are unchanged and it sits beyond the window, so the
    // viewport's own child-geometry cache would answer for it — collapsed —
    // without ever walking to the poisoned box.
    run.owner_mut().mark_needs_layout(viewport);
    let _ = run.owner_mut().run_layout();
    assert_eq!(
        position.max_scroll_extent(),
        1200.0,
        "a cache built by a degraded pass must not be served as healthy"
    );
}

// ============================================================================
// RenderAlign harness tests
// ============================================================================

// BOTTOM_RIGHT alignment: free space = 60×60 → offset = (60,60).
fn harness_align_bottom_right_places_child_at_free_space() {
    let run = RenderTester::mount(
        box_node(RenderAlign::new(Alignment::BOTTOM_RIGHT))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("child")),
    )
    .with_size(Size::new(100.0, 100.0))
    .run_layout();

    assert_eq!(run.offset(run.id("child")), Offset::new(60.0, 60.0));
}

// ============================================================================
// RenderCenter FIX tests (behaviors that changed in this PR)
// ============================================================================

// ============================================================================
// Wrap
// ============================================================================

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

    assert_eq!(run.box_geometry(run.root()), Size::new(80.0, 80.0));
    assert_eq!(run.offset(run.id("a")), Offset::ZERO);
    assert_eq!(run.offset(run.id("b")), Offset::new(40.0, 0.0));
    // Wrap proof: c must be on a new row, not overflowing the first.
    assert_eq!(
        run.offset(run.id("c")),
        Offset::new(0.0, 40.0),
        "c must wrap to a second run, not overflow the first",
    );
}

// ── RenderWrap dry layout ─────────────────────────────────────────────────────

// ============================================================================
// RenderIntrinsicWidth
// ============================================================================

// ---- RenderIntrinsicWidth shrink-wrapping -----------------------------------

/// Shrink-wrapping width.
fn harness_intrinsic_width_shrink_wrapping_width_oracle() {
    let mut run = RenderTester::mount(
        box_node(RenderIntrinsicWidth::unconstrained())
            .child(box_node(RenderTestBox::new(10.0, 100.0, 20.0, 200.0)).label("child")),
    )
    .with_constraints(BoxConstraints::new(5.0, 500.0, 8.0, 800.0))
    .run_layout();

    let root = run.root();
    let child = run.id("child");
    assert_eq!(run.box_geometry(root), Size::new(100.0, 110.0));
    assert_eq!(run.box_geometry(child), Size::new(100.0, 110.0));

    for h in [0.0, 10.0, 80.0, f64::INFINITY] {
        assert_eq!(run.min_intrinsic_width(root, h), 100.0, "min width @ h={h}");
        assert_eq!(run.max_intrinsic_width(root, h), 100.0, "max width @ h={h}");
        assert_eq!(
            run.min_intrinsic_height(root, h),
            20.0,
            "min height @ w={h}"
        );
        assert_eq!(
            run.max_intrinsic_height(root, h),
            200.0,
            "max height @ w={h}"
        );
    }
}

// ---- Slice-2 milestone: dry == committed for filling child ----------------

// ---- Slice-1 channel proof ------------------------------------------------

/// Verify that `BoxDryLayoutCtx::child_max_intrinsic_width` (the new intrinsic
/// dry-intrinsics channel) routes through the real memoized
/// `intrinsic_query` and returns the same value as a standalone
/// `max_intrinsic_width` call on the child.
///
/// This test uses a thin proxy whose `compute_dry_layout` records the
/// intrinsic it receives so the harness can assert equality.  It is
/// GREEN after Slice 1 (channel wired) and would be RED before it
/// (the accessor did not exist).
fn harness_dry_layout_child_intrinsic_channel_matches_standalone_query() {
    use std::sync::{Arc, Mutex};

    use flui_foundation::Single;
    use flui_rendering::{
        constraints::BoxConstraints,
        context::{BoxDryLayoutCtx, BoxIntrinsicsCtx},
        parent_data::BoxParentData,
        traits::RenderBox,
    };

    // Shared cell: `compute_dry_layout` writes the child intrinsic it observed.
    let captured: Arc<Mutex<f64>> = Arc::new(Mutex::new(f64::NAN));

    // Inline proxy whose only job is to expose the child's max-intrinsic-width
    // during a dry-layout pass.
    #[derive(Debug)]
    struct IntrinsicCapture {
        captured: Arc<Mutex<f64>>,
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
            _height: f64,
            _ctx: &mut BoxIntrinsicsCtx<'_>,
        ) -> f64 {
            0.0
        }

        fn compute_dry_layout(
            &self,
            constraints: BoxConstraints,
            ctx: &mut BoxDryLayoutCtx<'_>,
        ) -> Size {
            // Read the child's max intrinsic width through the new channel.
            let via_channel = ctx.child_max_intrinsic_width(0, f64::INFINITY);
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
    let constraints = BoxConstraints::new(0.0, 500.0, 0.0, 300.0);
    run.dry_layout(run.root(), constraints);

    let via_channel = *captured.lock().unwrap();
    assert!(
        !via_channel.is_nan(),
        "compute_dry_layout was not called — channel not exercised"
    );

    // The standalone query must agree with what the channel reported.
    let standalone = run.max_intrinsic_width(flex_id, f64::INFINITY);
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

// ---- RenderIntrinsicHeight shrink-wrapping ----------------------------------

/// Shrink-wrapping height.
fn harness_intrinsic_height_shrink_wrapping_height_oracle() {
    let mut run = RenderTester::mount(
        box_node(RenderIntrinsicHeight::new())
            .child(box_node(RenderTestBox::new(10.0, 100.0, 20.0, 200.0)).label("child")),
    )
    .with_constraints(BoxConstraints::new(5.0, 500.0, 8.0, 800.0))
    .run_layout();

    let root = run.root();
    assert_eq!(run.box_geometry(root), Size::new(55.0, 200.0));

    for w in [0.0, 10.0, 80.0, f64::INFINITY] {
        assert_eq!(run.min_intrinsic_width(root, w), 10.0, "min width @ h={w}");
        assert_eq!(run.max_intrinsic_width(root, w), 100.0, "max width @ h={w}");
        assert_eq!(
            run.min_intrinsic_height(root, w),
            200.0,
            "min height @ w={w}"
        );
        assert_eq!(
            run.max_intrinsic_height(root, w),
            200.0,
            "max height @ w={w}"
        );
    }
}

// ============================================================================
// RenderConstrainedOverflowBox
// ============================================================================

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
        Size::new(200.0, 200.0),
        "OverflowBoxFit::Max must claim all available space",
    );
}

// ============================================================================
// RenderSizedOverflowBox
// ============================================================================

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
        Size::new(40.0, 40.0),
        "child must be laid out under incoming constraints, not the requested size",
    );
}

// ============================================================================
// RenderConstraintsTransformBox
// ============================================================================

fn harness_constraints_transform_box_reports_overflow_when_child_exceeds_own_size() {
    let mut run = RenderTester::mount(
        box_node(RenderConstraintsTransformBox::new(
            Alignment::CENTER,
            None,
            Clip::None,
        ))
        .child(box_node(RenderColoredBox::red(200.0, 200.0)).label("child")),
    )
    .with_constraints(BoxConstraints::new(0.0, 50.0, 0.0, 50.0))
    .run_layout();

    let root = run.root();
    let node = run
        .owner_mut()
        .render_tree_mut()
        .get_mut(root)
        .and_then(|node| node.downcast_render_object_mut::<RenderConstraintsTransformBox>())
        .expect("root should be a RenderConstraintsTransformBox");
    assert!(
        node.has_visual_overflow(),
        "a 200x200 child inside a 50x50 box must be reported as overflowing"
    );
}

// ============================================================================
// RenderRotatedBox
// ============================================================================

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
        Size::new(40.0, 60.0),
        "1 quarter turn must swap child width↔height for the parent-reported size",
    );
}

// ============================================================================
// RenderFlow — paint-time transform layout
// ============================================================================

/// Translates child `i` by `i * step` along x. Mirrors
/// `flow_delegate.rs`'s `LinearFlowDelegate` test fixture.
#[derive(Debug)]
struct StepFlowDelegate {
    step: f64,
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
            context.paint_child(i, Matrix4::translation(i as f64 * self.step, 0.0, 0.0));
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
                Matrix4::translation(i as f64 * 50.0, 0.0, 0.0)
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

fn harness_flow_paints_children_in_delegate_order_under_per_child_transform_layers() {
    let run = RenderTester::mount(
        box_node(RenderFlow::new(Arc::new(StepFlowDelegate { step: 30.0 })))
            .child(box_node(RenderColoredBox::red(20.0, 20.0)).label("a"))
            .child(box_node(RenderColoredBox::green(20.0, 20.0)).label("b"))
            .child(box_node(RenderColoredBox::blue(20.0, 20.0)).label("c")),
    )
    .with_size(Size::new(200.0, 50.0))
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
    // transform (which `RenderObject::paint_effects().transform` already
    // supports and would show up as a single Transform layer regardless of
    // child count).
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

fn harness_flow_degenerate_transform_is_never_hit_but_siblings_still_are() {
    let run = RenderTester::mount(
        box_node(RenderFlow::new(Arc::new(DegenerateFlowDelegate)))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("zeroed"))
            .child(box_node(RenderColoredBox::green(40.0, 40.0)).label("normal")),
    )
    .with_size(Size::new(200.0, 100.0))
    .run_frame();

    // The zero-scale child collapses to a single point; no finite position
    // can hit it, and its inverse doesn't exist so `RenderFlow::hit_test`
    // must skip it outright rather than panicking or matching everything.
    assert_eq!(run.hit(0.0, 0.0), [] as [flui_foundation::RenderId; 0]);
    assert_eq!(run.hit(10.0, 10.0), [] as [flui_foundation::RenderId; 0]);
    // The sibling at a real translation is unaffected by child 0's
    // degenerate transform.
    assert_eq!(run.hit_first(70.0, 20.0), Some(run.id("normal")));
}

// ============================================================================
// RenderTable
// ============================================================================

/// Tight width (forces `_computeColumnWidths`' pass 2 to grow the Flex column
/// to fill the remainder), loose height (so the table's own height comes from
/// content, not the incoming constraints).
fn table_tight_width_loose_height(width: f64, max_height: f64) -> BoxConstraints {
    BoxConstraints::new(width, width, 0.0, max_height)
}

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
        Size::new(200.0, 45.0),
        "table size must be the sum of resolved column widths (200) and row heights (30+15)",
    );

    assert_eq!(run.offset(run.id("a")), Offset::new(0.0, 0.0));
    assert_eq!(run.box_geometry(run.id("a")), Size::new(50.0, 20.0));

    assert_eq!(run.offset(run.id("b")), Offset::new(50.0, 0.0));
    assert_eq!(run.box_geometry(run.id("b")), Size::new(150.0, 30.0));

    assert_eq!(run.offset(run.id("c")), Offset::new(0.0, 30.0));
    assert_eq!(run.box_geometry(run.id("c")), Size::new(50.0, 15.0));

    assert_eq!(run.offset(run.id("d")), Offset::new(50.0, 30.0));
    assert_eq!(run.box_geometry(run.id("d")), Size::new(150.0, 10.0));

    assert_descendant_properties(
        &run.diagnostics(),
        "RenderTable",
        &["column_count", "default_vertical_alignment"],
    );
}

fn harness_table_paints_row_decoration_then_children_then_border_in_order() {
    // 1 row x 2 columns, uniform border (so the outer edge is one DrawDRRect)
    // plus a solid `vertical_inside` (so there's exactly one interior line —
    // no `horizontal_inside` line since there's only 1 row).
    let border = TableBorder::all(BorderSide::new(Color::BLUE, 2.0, BorderStyle::Solid));
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
    assert_eq!(
        commands
            .iter()
            .map(|command| command.line.as_str())
            .filter(|line| matches!(*line, "Save" | "Restore"))
            .collect::<Vec<_>>(),
        [
            "Save", "Restore", "Save", "Restore", "Save", "Restore", "Save", "Restore"
        ],
        "decoration, each child, and border must retain separate paint scopes",
    );
    let commands: Vec<_> = commands
        .iter()
        .filter(|command| !matches!(command.line.as_str(), "Save" | "Restore"))
        .collect();
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

fn harness_table_baseline_alignment_lines_up_cells_on_their_shared_baseline() {
    // Both cells opt into `Baseline` alignment; the table-wide baseline
    // (`before_baseline`) is the max reported baseline in the row (30, from
    // "tall"). "short" (baseline 10) must be pushed down by 30 - 10 = 20 so
    // its own baseline coincides with "tall"'s at y=30 from the row top.
    let run = RenderTester::mount(
        box_node(RenderTable::new(2).with_text_baseline(Some(TextBaseline::Alphabetic)))
            .child(
                box_node(RenderBaseline::new(TextBaseline::Alphabetic, 30.0))
                    .child(box_node(RenderColoredBox::red(20.0, 10.0)))
                    .with_table_parent_data(
                        TableCellParentData::zero()
                            .with_alignment(TableCellVerticalAlignment::Baseline),
                    )
                    .label("tall"),
            )
            .child(
                box_node(RenderBaseline::new(TextBaseline::Alphabetic, 10.0))
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

    assert_eq!(run.offset(run.id("tall")).dy, 0.0);
    assert_eq!(run.offset(run.id("short")).dy, 20.0);
    assert_eq!(
        run.box_geometry(run.root()).height,
        30.0,
        "row height must be the table-wide baseline distance (30) since \
         after_baseline is 0 for both cells",
    );
}

// ============================================================================
// RenderAnimatedSize
// ============================================================================
//
// Every test below constructs its own `AnimationController` (a fresh,
// never-pumped `UpdateScheduler`) and, where the test needs to
// drive the retarget animation across frames, keeps a `Clone` of it (`driver`)
// to call `tick_at(seconds_since_the_current_run_started)` directly —
// mirroring how `flui-animation`'s own controller tests and `Vsync::tick_all`
// drive deterministic virtual time, with no `thread::sleep`. `run.pump()`
// only re-runs the render pipeline; it does not itself advance the
// controller, so a value-listener-driven `mark_needs_layout` (buffered by
// `attach`) is drained on the very next `pump()`/`run_frame()` after a tick.

fn animated_size_controller(ms: u64) -> (AnimationController, AnimationController) {
    let controller = AnimationController::new(Duration::from_millis(ms), &UpdateScheduler::new());
    let driver = controller.clone();
    (controller, driver)
}

fn assert_size_approx(actual: Size, expected: Size, eps: f64, what: &str) {
    assert!(
        (actual.width - expected.width).abs() < eps
            && (actual.height - expected.height).abs() < eps,
        "{what}: expected ~{expected:?} (±{eps}), got {actual:?}",
    );
}

fn harness_render_animated_size_interpolates_over_several_frames_not_snap() {
    let (controller, driver) = animated_size_controller(100);
    let ro = RenderAnimatedSize::new(
        controller,
        ArcCurve::new(Curves::Linear),
        Alignment::CENTER,
        Clip::HardEdge,
    );

    let mut run = RenderTester::mount(
        box_node(ro)
            .label("root")
            .child(box_node(RenderColoredBox::red(10.0, 10.0)).label("child")),
    )
    .run_frame();
    assert_eq!(run.box_geometry(run.root()), Size::new(10.0, 10.0));

    // Grow the child: Stable -> Changed (begin = last committed size = 10,
    // end = 50), controller restarts at t = 0.
    run.update::<RenderColoredBox>(run.id("child"), |b| {
        assert_eq!(
            b.set_preferred_size(Size::new(50.0, 50.0)),
            flui_rendering::RenderUpdateImpact::LAYOUT
        );
    });
    run.pump();
    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(10.0, 10.0),
        "the retarget frame itself reports t=0 (still the begin size) — no snap to end",
    );

    // Tick the controller to known fractions of the 100ms run and confirm the
    // reported size actually interpolates (hand-computed against
    // Tween::transform), not just holds or jumps straight to the target.
    driver.tick_at(0.025); // 25ms of 100ms => t=0.25
    run.pump();
    assert_size_approx(
        run.box_geometry(run.root()),
        Size::new(20.0, 20.0), // 10 + 0.25 * (50-10)
        0.5,
        "t=0.25",
    );

    driver.tick_at(0.05); // t=0.5
    run.pump();
    assert_size_approx(
        run.box_geometry(run.root()),
        Size::new(30.0, 30.0),
        0.5,
        "t=0.5",
    );

    driver.tick_at(0.075); // t=0.75
    run.pump();
    assert_size_approx(
        run.box_geometry(run.root()),
        Size::new(40.0, 40.0),
        0.5,
        "t=0.75",
    );

    driver.tick_at(0.1); // t=1.0, run completes
    run.pump();
    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(50.0, 50.0),
        "a completed run must land exactly on the target size",
    );
}

fn harness_render_animated_size_retarget_mid_flight_has_no_discontinuous_jump() {
    let (controller, driver) = animated_size_controller(100);
    let ro = RenderAnimatedSize::new(
        controller,
        ArcCurve::new(Curves::Linear),
        Alignment::CENTER,
        Clip::HardEdge,
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
        assert_eq!(
            b.set_preferred_size(Size::new(50.0, 50.0)),
            flui_rendering::RenderUpdateImpact::LAYOUT
        );
    });
    run.pump();
    driver.tick_at(0.05); // t=0.5 of the 10->50 span
    run.pump();
    let mid_flight_size = run.box_geometry(run.root());
    assert_size_approx(
        mid_flight_size,
        Size::new(30.0, 30.0),
        0.5,
        "midpoint of the first span",
    );

    // Second retarget while STILL mid-flight (the 10->50 run has not reached
    // t=1.0): begin must be the CURRENT committed value (continuous), not a
    // degenerate collapse — this is the Stable->Changed formula, distinct
    // from the Changed->Unstable degenerate-collapse case tested at the unit
    // level.
    run.update::<RenderColoredBox>(run.id("child"), |b| {
        assert_eq!(
            b.set_preferred_size(Size::new(90.0, 90.0)),
            flui_rendering::RenderUpdateImpact::LAYOUT
        );
    });
    run.pump();
    let retarget_frame_size = run.box_geometry(run.root());

    assert_eq!(
        retarget_frame_size, mid_flight_size,
        "retargeting mid-flight must begin exactly at the last committed \
         size — no discontinuous jump on the retarget frame itself",
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
// .min(0.0)` line) computed the wrong sign relative to
// `RenderShrinkWrappingViewport::attempt_layout`, its sibling
// (already correct: `overlap: corrected_offset.min(0.0)`) — confirmed
// empirically (a Pinned header at scroll_offset=300 reported
// `paint_origin == -300.0`, i.e. `overlap == -300.0`, where a correct
// top-anchored forward viewport must report `overlap == 0.0` for its first
// sliver). **Fixed**: the formula now is right for both the no-reverse-group
// case and the leading-negative-child case (which forces `overlap` to `0.0`
// for both sequences); no test pins the corrected sign. It was a pre-existing
// defect, not something introduced by this
// family's pass — no existing sliver in the catalog read `constraints.overlap`
// in a way any prior test asserted on, so it had zero coverage until these
// headers exercised it and it also would have affected
// `RenderSliverFillRemainingAndOverscroll`/`RenderSliverFillRemainingWithScrollable`,
// which already read `constraints.overlap`. The tests below still avoid
// depending on `overlap`-derived quantities through a real viewport (they
// assert `paint_extent`/`effective_scroll_offset`/`max_scroll_obstruction_extent`,
// none of which round-trip through `overlap` at scroll_offset > 0 in these
// specific scenarios); no test covers the stretch-configuration formulas
// that DO need a specific `overlap`.
//
// A second, separate finding (since fixed): `RenderTester::mount` used to
// never call `RenderObject::attach` for a Sliver child. Box children went
// through `PipelineOwner::insert_child_render_object`, which calls
// `attach_inserted_node` — but Sliver children were inserted via the
// low-level `render_tree_mut().insert_sliver_child(...)`
// (`crates/flui-rendering/src/storage/tree.rs`), which did not.
// `crate::testing::tree::mount_child` now inserts Sliver children via the
// new `PipelineOwner::insert_sliver_child_render_object` (the
// Sliver-protocol counterpart of `insert_child_render_object`; see
// `crates/flui-rendering/tests/attach_detach_lifecycle.rs` for the
// regression coverage).

fn viewport_multi_with_scroll(
    offset: f64,
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
    .with_size(Size::new(300.0, 400.0))
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
        0.0,
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
    .with_size(Size::new(300.0, 400.0))
    .run_layout();

    let header_id = run.id("header");
    let vp_id = run.id("viewport");

    assert_eq!(
        run.sliver_geometry(header_id).max_scroll_obstruction_extent,
        40.0,
        "max_scroll_obstruction_extent must report min_extent",
    );
    assert_eq!(run.offset(run.id("child")).dy, 0.0);

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
        0.0,
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
    .with_size(Size::new(300.0, 400.0))
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
    .with_size(Size::new(300.0, 400.0))
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
        0.0,
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
        0.0,
        "child_main_axis_position stays 0.0 mid-reveal too, unlike plain Floating",
    );
}

// ============================================================================
// RenderLayoutBuilder (ADR-0017) — the render half of the
// build-during-layout seam. It publishes constraints; it never builds.
//
// These assertions encode the algorithm recorded in ADR-0017.
// ============================================================================

/// A layout pass must publish the **real** incoming constraints — not a
/// placeholder, and not a default. This is the regression that catches a
/// reprise of the pre-rewrite `LayoutBuilder`, whose builder was handed
/// `BoxConstraints::UNCONSTRAINED` (commit `bb58a8fa`).
fn harness_layout_builder_publishes_the_real_incoming_constraints() {
    let cell = Arc::new(LayoutConstraintsCell::new());
    let incoming = BoxConstraints::new(10.0, 120.0, 20.0, 90.0);

    let _run = RenderTester::mount(
        box_node(RenderLayoutBuilder::new(Arc::clone(&cell)))
            .child(box_node(RenderColoredBox::green(30.0, 40.0)).label("child")),
    )
    .with_constraints(incoming)
    .run_layout();

    assert_eq!(
        cell.constraints(),
        Some(incoming),
        "the builder must see the exact constraints the parent imposed"
    );
    assert!(
        cell.needs_build(),
        "the first-ever publish must schedule the builder"
    );
}

// ============================================================================
// Catalog guard — every exported render type must be exercised above
// ============================================================================

#[test]
fn catalog_covers_every_render_object_name() {
    let source = include_str!("render_object_harness.rs").replace('\r', "");
    for &type_name in RENDER_OBJECT_TYPES {
        // A row body ends at the first closing brace in column 0; the family
        // tables and these guards sit after the last row and must not count.
        let covered = source.split("\nfn harness_").skip(1).any(|chunk| {
            chunk
                .split("\n}\n")
                .next()
                .is_some_and(|body| body.contains(type_name))
        });
        assert!(
            covered,
            "{type_name} must appear in at least one `fn harness_*` family-table row",
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

// ── RenderTheater ─────────────────────────────────────────────────────────────

/// The leading `skip_count` children are offstage: not laid out, not painted,
/// not hit-tested. Paint and hit-test order both start at the first onstage
/// child, and layout only walks paint order.
fn harness_theater_skips_leading_children_in_layout_paint_and_hit_test() {
    let run = RenderTester::mount(
        box_node(RenderTheater::new().with_skip_count(1))
            .child(box_node(RenderColoredBox::red(40.0, 40.0)).label("offstage"))
            .child(box_node(RenderColoredBox::green(30.0, 30.0)).label("onstage")),
    )
    .with_constraints(loose(200.0))
    .run_frame();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(200.0, 200.0),
        "skipping children must not change the theater's own size",
    );
    assert_eq!(
        run.try_box_geometry(run.id("offstage")),
        None,
        "the skipped child must never be laid out — it has no committed geometry",
    );
    assert_eq!(
        run.box_geometry(run.id("onstage")),
        Size::new(200.0, 200.0),
        "the onstage child is still tight to the theater's size",
    );
    assert_eq!(
        run.hit_first(10.0, 10.0),
        Some(run.id("onstage")),
        "the skipped child must not be hit-testable",
    );

    let painted = run
        .display_commands()
        .into_iter()
        .map(|cmd| cmd.line)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        painted.contains("#00FF00FF"),
        "onstage green child must paint; commands:\n{painted}",
    );
    assert!(
        !painted.contains("#FF0000FF"),
        "offstage red child must not paint; commands:\n{painted}",
    );
}

// ── Ancestor paint transforms (ADR-0021) ──────────────────────────────────────
//
// `PipelineOwner::transform_to` composes one
// `RenderObject::apply_paint_transform` per level. The default body is the paint
// pipeline's own composition — `paint_effects(size).transform` then a
// translation by the child's committed offset — so `RenderRotatedBox` needs
// **no override**: its transform is reported entirely through `paint_effects`,
// and the default feeds it. `RenderTransform` and `RenderFittedBox` DO override
// `apply_paint_transform`, for two different reasons: `RenderTransform` leaves
// `paint_effects().transform` at `None` for a pure translation (its no-layer
// fast path applies the offset in `paint`), so the mapping must carry the
// matrix in both branches; `RenderFittedBox` opens its transform layer inside
// `paint` (so its clip can sit outside it) and reports none through
// `paint_effects` at all. `RenderFractionalTranslation` and
// `RenderFlow` also need one, because their paint bypasses the committed offset
// altogether (`paint_child_at` / a per-child transform scope). These tests pin
// all four overriding shapes plus the one default-composition case.

/// Nesting the two override cases inside a transforming ancestor: the walk must
/// compose every level, outermost first.
fn harness_transform_to_composes_a_whole_chain() {
    let run = RenderTester::mount(
        box_node(RenderTransform::uniform_scale(2.0))
            .label("root")
            .child(
                box_node(RenderFractionalTranslation::translated(
                    TranslationFraction::new(0.5, 0.0),
                ))
                .label("shift")
                .child(box_node(RenderColoredBox::red(20.0, 20.0)).label("child")),
            ),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    let transform = run
        .owner()
        .transform_to(run.id("child"), run.root())
        .expect("descendant");

    // `shift` is 20×20, so it moves the child +10 in x. `root` is 20×20 and
    // scales ×2 about its centre: x' = 2·(10) − 10 = 10, y' = 2·0 − 10 = −10.
    let (x, y) = transform.transform_point(0.0, 0.0);
    assert_transform_point(x, y, 10.0, -10.0, "scale ∘ fractional translation");
}

/// Asserts a transformed point, with the tolerance a 4×4 float matrix needs
/// (a quarter turn leaves ~2e-6 of residue on the zeroed axis).
fn assert_transform_point(x: f64, y: f64, expected_x: f64, expected_y: f64, what: &str) {
    assert!(
        (x - expected_x).abs() < 1e-4 && (y - expected_y).abs() < 1e-4,
        "{what}: expected ({expected_x}, {expected_y}), got ({x}, {y})",
    );
}

// ── RenderSubtreeAnchor (ADR-0021) ───────────────────────────────────────────
//
// The anchor's whole job is identity: publish its own `RenderId` while mounted,
// clear it when it leaves. `attach(RenderInvalidationHandle)` is the first — and only —
// hook where that id exists (`RenderInvalidationHandle::id()`); `detach()` is its mirror.
// Everything else must be invisible.

/// `attach` publishes the render object's **real** id — the one the pipeline
/// knows it by, not a fabricated or zeroed placeholder.
fn harness_subtree_anchor_attach_publishes_the_real_render_id() {
    let anchor = SubtreeAnchor::new();
    assert_eq!(anchor.get(), None, "an anchor names nothing before mount");
    assert!(!anchor.is_anchored());

    let run = RenderTester::mount(
        box_node(RenderSubtreeAnchor::new(anchor.clone()))
            .label("anchor")
            .child(box_node(RenderColoredBox::red(40.0, 24.0)).label("child")),
    )
    .with_constraints(loose(200.0))
    .run_layout();

    assert_eq!(
        anchor.get(),
        Some(run.id("anchor")),
        "the published id must be the anchor node's own RenderId"
    );
    assert!(anchor.is_anchored());
}

fn harness_sliver_main_axis_group_composes_scroll_extents_and_places_children() {
    let run = RenderTester::mount(viewport(
        sliver_node(RenderSliverMainAxisGroup::new())
            .label("group")
            .child(
                sliver_node(RenderSliverToBoxAdapter::new())
                    .label("first")
                    .child(box_node(RenderSizedBox::fixed(300.0, 80.0))),
            )
            .child(
                sliver_node(RenderSliverToBoxAdapter::new())
                    .label("second")
                    .child(box_node(RenderSizedBox::fixed(300.0, 120.0))),
            ),
    ))
    .with_size(Size::new(300.0, 600.0))
    .run_layout();

    let group = run.sliver_geometry(run.id("group"));
    assert_eq!(
        group.scroll_extent, 200.0,
        "the group's scroll extent is the sum of its children's (80 + 120)",
    );
    assert_eq!(
        group.paint_extent, 200.0,
        "everything fits: paint extent equals the composed extent",
    );
    assert!(group.visible, "a painting group is visible");
    // The second child is placed after the first along the main axis.
    assert_eq!(run.offset(run.id("first")).dy, 0.0);
    assert_eq!(run.offset(run.id("second")).dy, 80.0);
    assert_has_committed_geometry(
        run.diagnostics()
            .find_descendant("RenderSliverMainAxisGroup")
            .expect("group in diagnostics"),
    );
}

/// `IntrinsicHeight` cells are measured AND stretched; `Fill` cells are only
/// stretched. That is the whole difference between the two, and it is what
/// decides how tall the row is.
///
/// `IntrinsicHeight` is grouped with `Top`/`Middle`/`Bottom` in the measure
/// pass and with `Fill` in the position pass.
fn harness_table_intrinsic_height_measures_the_row_then_stretches_every_cell_to_it() {
    let run = RenderTester::mount(
        box_node(
            RenderTable::new(2)
                .with_default_vertical_alignment(TableCellVerticalAlignment::IntrinsicHeight),
        )
        .child(box_node(RenderColoredBox::red(100.0, 40.0)).label("short"))
        .child(box_node(RenderColoredBox::green(100.0, 90.0)).label("tall")),
    )
    .with_constraints(table_tight_width_loose_height(200.0, 800.0))
    .run_frame();

    assert_eq!(
        run.box_geometry(run.root()),
        Size::new(200.0, 90.0),
        "the row is as tall as its tallest cell — the short cell was measured, \
         so it took part in deciding that, and the tall one set it",
    );
    assert_eq!(
        run.box_geometry(run.id("short")).height,
        90.0,
        "and the short cell is then stretched to the row it helped size",
    );
    assert_eq!(run.box_geometry(run.id("tall")).height, 90.0);
    assert_eq!(run.offset(run.id("short")), Offset::new(0.0, 0.0));
    assert_eq!(run.offset(run.id("tall")), Offset::new(100.0, 0.0));
}

// ── The placed-generation gate reaches semantics too ──────────────────────

/// Lays out and positions children `0..laid_out`, leaving the rest untouched —
/// the shape of any virtualising parent, and the only way to observe a child
/// that a LATER pass stopped laying out.
#[derive(Debug)]
struct LaysOutFirstN {
    laid_out: usize,
}

impl flui_foundation::Diagnosticable for LaysOutFirstN {}

impl RenderBox for LaysOutFirstN {
    type Arity = flui_foundation::Variable;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<
            '_,
            flui_foundation::Variable,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> Size {
        let constraints = *ctx.constraints();
        let count = ctx.child_count().min(self.laid_out);
        for i in 0..count {
            let size = ctx.layout_child(i, constraints);
            ctx.position_child(i, Offset::new(0.0, i as f64 * size.height));
        }
        constraints.constrain(Size::new(100.0, 100.0))
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<
            '_,
            flui_foundation::Variable,
            flui_rendering::parent_data::BoxParentData,
        >,
    ) -> bool {
        false
    }
}

/// A child dropped from a later layout pass contributes no semantics.
///
/// The third walk, after paint and hit-test. A screen reader sent to a row
/// that is no longer laid out lands somewhere with nothing on it — worse than
/// not announcing it.
///
/// This gate was deferred once, because `harness_merge_semantics_collapses_
/// descendant_boundaries` lost a descendant's `is_button` under it. That is no
/// longer true, and the reason is the point: the stamp now carries the issuing
/// parent's identity, so a child its parent NEVER laid out reads as placed
/// (`placed_by == 0`) rather than as skipped. The gate therefore excludes only
/// children a parent once laid out and then dropped — this case — and leaves
/// alone the ones it never touched, which is what that harness fixture has.
fn harness_placed_generation_gate_excludes_a_dropped_child_from_semantics() {
    let labelled = |label: &str| {
        box_node(
            RenderSemanticsAnnotations::new(SemanticsProperties::new().with_label(label))
                .with_container(true),
        )
        .child(box_node(RenderSizedBox::new(Some(40.0), Some(20.0))))
    };

    let run = RenderTester::mount(
        box_node(LaysOutFirstN { laid_out: 2 })
            .label("host")
            .child(labelled("kept"))
            .child(labelled("dropped")),
    )
    .with_constraints(loose(200.0))
    .with_semantics_enabled()
    .run_to_semantics();

    // Pass one lays both out, so the second is stamped and announced. Without
    // this the second child was never stamped at all, which reads as PLACED —
    // and the test would pass with the gate reverted, as its first draft did.
    let host = run.root();
    let announced_first: Vec<String> = run
        .semantics_owner()
        .expect("semantics enabled")
        .tree()
        .iter()
        .filter_map(|(_, node)| node.label().map(ToString::to_string))
        .collect();
    assert!(
        announced_first.iter().any(|l| l.contains("dropped")),
        "precondition: both rows are announced while both are laid out; got \
         {announced_first:?}"
    );

    // Pass two lays out only the first.
    let run = run.edit_and_run_again::<LaysOutFirstN>(host, |object| object.laid_out = 1);

    let owner = run.semantics_owner().expect("semantics enabled");
    let announced: Vec<String> = owner
        .tree()
        .iter()
        .filter_map(|(_, node)| node.label().map(ToString::to_string))
        .collect();

    assert!(
        announced.iter().any(|l| l.contains("kept")),
        "the child this pass laid out is announced; got {announced:?}"
    );
    assert!(
        !announced.iter().any(|l| l.contains("dropped")),
        "a child this pass did not lay out must not be announced at a rect \
         from a pass that no longer holds; got {announced:?}"
    );
}

/// An entry offstage from its VERY FIRST pass publishes no semantics.
///
/// This is the case the placed-generation stamp provably cannot reach, and it
/// is why the per-child visitor exists. The stamp excludes a child a parent
/// *stopped* laying out; a child skipped from pass one was never stamped, and
/// an unstamped child reads as placed by design — the stamp may only remove a
/// child that demonstrably fell out, or a parent laying out through a path of
/// its own would hide its whole subtree.
///
/// Concretely: an app that STARTS with an opaque route above another never
/// lays the lower one out, so a screen reader announced a route the user could
/// neither see nor touch. Push-then-cover was already handled (the entry was
/// laid out once, so its stamp goes stale); this is the other half.
///
/// Red without `RenderTheater::visits_child_for_semantics`: the covered entry
/// is announced.
fn harness_theater_offstage_from_the_first_pass_publishes_no_semantics() {
    let labelled = |label: &str| {
        box_node(
            RenderSemanticsAnnotations::new(SemanticsProperties::new().with_label(label))
                .with_container(true),
        )
        .child(box_node(RenderSizedBox::new(Some(40.0), Some(20.0))))
    };

    // `skip_count = 1` from the start: the bottom entry is never laid out, so
    // nothing ever stamps it.
    let run = RenderTester::mount(
        box_node(RenderTheater::new().with_skip_count(1))
            .child(labelled("covered"))
            .child(labelled("visible")),
    )
    .with_constraints(loose(200.0))
    .with_semantics_enabled()
    .run_to_semantics();

    let announced: Vec<String> = run
        .semantics_owner()
        .expect("semantics enabled")
        .tree()
        .iter()
        .filter_map(|(_, node)| node.label().map(ToString::to_string))
        .collect();

    assert!(
        announced.iter().any(|l| l.contains("visible")),
        "the presented entry must still be announced; got {announced:?}"
    );
    assert!(
        !announced.iter().any(|l| l.contains("covered")),
        "an entry offstage from its first pass must publish no semantics — a \
         screen reader would otherwise find a route the user cannot see or \
         touch; got {announced:?}"
    );
}

/// A right-to-left `Row` lays its children out from the right.
///
/// `RenderFlex::flip_main_axis` is consulted only by a horizontal flex, and
/// until now nothing in this crate set `TextDirection::Rtl` at all — the whole
/// suite passes with the flip hard-coded to `false`, so the behaviour lived
/// entirely on widget-level coverage one layer up. This pins it where it is
/// implemented.
fn harness_flex_row_rtl_lays_children_out_from_the_right() {
    let row = |direction| {
        RenderTester::mount(
            box_node(
                RenderFlex::row()
                    .with_main_axis_alignment(MainAxisAlignment::Start)
                    .with_text_direction(direction),
            )
            .child(box_node(RenderColoredBox::red(50.0, 20.0)).label("first"))
            .child(box_node(RenderColoredBox::red(50.0, 20.0)).label("second")),
        )
        .with_size(Size::new(300.0, 100.0))
        .run_layout()
    };

    let ltr = row(TextDirection::Ltr);
    let (ltr_first, ltr_second) = (
        ltr.offset(ltr.id("first")).dx,
        ltr.offset(ltr.id("second")).dx,
    );
    assert_eq!(
        (ltr_first, ltr_second),
        (0.0, 50.0),
        "premise: left-to-right packs from the left, in declaration order"
    );

    let rtl = row(TextDirection::Rtl);
    let (rtl_first, rtl_second) = (
        rtl.offset(rtl.id("first")).dx,
        rtl.offset(rtl.id("second")).dx,
    );
    assert_eq!(
        (rtl_first, rtl_second),
        (250.0, 200.0),
        "right-to-left packs from the right, so the FIRST child sits rightmost \
         — the declaration order is unchanged, the axis is reversed"
    );
    assert!(
        rtl_first > rtl_second,
        "and the two must not merely be translated together: the first child \
         has to end up further right than the second"
    );
}

/// A right-to-left horizontal `Wrap` packs its run against the RIGHT edge.
///
/// `RenderWrap` documented "no axis flipping" until this landed, so nothing in
/// the crate set a direction on it.
///
/// The children are deliberately UNEQUAL (40 then 80) and the container leaves
/// free space. Both matter, and an earlier version of this test had neither:
///
/// * equal children hide a size/index mismatch — positioning child *i* while
///   advancing the cursor by child *j*'s extent overlaps them, and two same-size
///   children make that invisible;
/// * with no free space, `Start` lands on the same coordinate whether or not
///   the alignment itself is flipped, so the test cannot see that half at all.
fn harness_wrap_horizontal_rtl_packs_its_run_against_the_right_edge() {
    let wrap = |direction| {
        RenderTester::mount(
            box_node(
                RenderWrap::new()
                    .with_direction(Axis::Horizontal)
                    .with_text_direction(direction),
            )
            .child(box_node(RenderColoredBox::red(40.0, 20.0)).label("narrow"))
            .child(box_node(RenderColoredBox::red(80.0, 20.0)).label("wide")),
        )
        .with_size(Size::new(200.0, 100.0))
        .run_layout()
    };

    let ltr = wrap(TextDirection::Ltr);
    assert_eq!(
        (
            ltr.offset(ltr.id("narrow")).dx,
            ltr.offset(ltr.id("wide")).dx
        ),
        (0.0, 40.0),
        "premise: left-to-right packs against the LEFT edge in declaration \
         order, so the 40px child sits at 0 and the 80px one directly after it"
    );

    let rtl = wrap(TextDirection::Rtl);
    let (narrow, wide) = (
        rtl.offset(rtl.id("narrow")).dx,
        rtl.offset(rtl.id("wide")).dx,
    );
    assert_eq!(
        (narrow, wide),
        (160.0, 80.0),
        "right-to-left packs against the RIGHT edge: the run's 120px of content \
         starts at x=80, the FIRST child takes the rightmost 40px at x=160, and \
         the second sits immediately left of it"
    );
    // The two children must ABUT, not overlap. `narrow` spans [160,200) and
    // `wide` spans [80,160). Advancing the cursor by the wrong child's extent
    // is exactly what produces an overlap here.
    assert_eq!(
        wide + 80.0,
        narrow,
        "the wide child's right edge must meet the narrow child's left edge -- \
         a cursor advanced by the other child's extent overlaps them instead"
    );
}

fn harness_subtree_anchor_detach_preserves_replacement_publication() {
    use flui_rendering::pipeline::PipelineOwner;
    use flui_rendering::protocol::BoxProtocol;
    let anchor = SubtreeAnchor::new();
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let first = owner.insert::<BoxProtocol>(Box::new(RenderSubtreeAnchor::new(anchor.clone())));
    let second = owner.insert::<BoxProtocol>(Box::new(RenderSubtreeAnchor::new(anchor.clone())));
    assert_eq!(anchor.get(), Some(second));
    owner.remove_render_object(first);
    assert_eq!(anchor.get(), Some(second));
    owner.remove_render_object(second);
    assert_eq!(anchor.get(), None);
}

// ============================================================================
// Family tables: one `#[test]` per family, one row per render object
// ============================================================================

type Case = (&'static str, fn());

/// Runs every row of a family and names each failing row, so one broken
/// render object cannot hide the rest of its family.
fn run_family(family: &str, cases: &[Case]) {
    let mut failed = Vec::new();
    for &(name, case) in cases {
        if std::panic::catch_unwind(case).is_err() {
            failed.push(name);
        }
    }
    assert!(failed.is_empty(), "{family}: failing rows: {failed:?}");
}

#[test]
fn family_sizing() {
    run_family(
        "sizing",
        &[
            (
                "sized_box_forces_dimensions",
                harness_sized_box_forces_dimensions,
            ),
            (
                "aspect_ratio_enforces_ratio",
                harness_aspect_ratio_enforces_ratio,
            ),
            (
                "aspect_ratio_unbounded_minima_remain_finite",
                harness_aspect_ratio_unbounded_minima_remain_finite,
            ),
            (
                "aspect_ratio_infinite_minima_reject_without_panicking",
                harness_aspect_ratio_infinite_minima_reject_without_panicking,
            ),
            (
                "constrained_box_enforces_minimums",
                harness_constrained_box_enforces_minimums,
            ),
            (
                "constrained_box_preserves_large_finite_bound",
                harness_constrained_box_preserves_large_finite_bound,
            ),
            (
                "constrained_box_keeps_hundredth_quantization",
                harness_constrained_box_keeps_hundredth_quantization,
            ),
            (
                "limited_box_caps_unbounded_width_in_row",
                harness_limited_box_caps_unbounded_width_in_row,
            ),
            (
                "fitted_box_preserves_aspect_ratio_when_sizing_box",
                harness_fitted_box_preserves_aspect_ratio_when_sizing_box,
            ),
            (
                "fractionally_sized_box_applies_width_factor",
                harness_fractionally_sized_box_applies_width_factor,
            ),
            (
                "constrained_overflow_box_max_fit_claims_full_parent",
                harness_constrained_overflow_box_max_fit_claims_full_parent,
            ),
            (
                "sized_overflow_box_child_lays_out_under_incoming_constraints",
                harness_sized_overflow_box_child_lays_out_under_incoming_constraints,
            ),
            (
                "constraints_transform_box_reports_overflow_when_child_exceeds_own_size",
                harness_constraints_transform_box_reports_overflow_when_child_exceeds_own_size,
            ),
            (
                "rotated_box_odd_turns_swaps_axes",
                harness_rotated_box_odd_turns_swaps_axes,
            ),
            (
                "layout_builder_publishes_the_real_incoming_constraints",
                harness_layout_builder_publishes_the_real_incoming_constraints,
            ),
        ],
    );
}

#[test]
fn family_intrinsics() {
    run_family(
        "intrinsics",
        &[
            (
                "intrinsic_width_shrink_wrapping_width_oracle",
                harness_intrinsic_width_shrink_wrapping_width_oracle,
            ),
            (
                "intrinsic_width_preserves_size_with_tiny_step",
                harness_intrinsic_width_preserves_size_with_tiny_step,
            ),
            (
                "intrinsic_width_rounds_up_to_ordinary_step",
                harness_intrinsic_width_rounds_up_to_ordinary_step,
            ),
            (
                "dry_layout_child_intrinsic_channel_matches_standalone_query",
                harness_dry_layout_child_intrinsic_channel_matches_standalone_query,
            ),
            (
                "intrinsic_height_shrink_wrapping_height_oracle",
                harness_intrinsic_height_shrink_wrapping_height_oracle,
            ),
        ],
    );
}

#[test]
fn family_alignment() {
    run_family(
        "alignment",
        &[
            (
                "padding_deflates_child_offset",
                harness_padding_deflates_child_offset,
            ),
            ("center_centers_child", harness_center_centers_child),
            (
                "align_bottom_right_places_child_at_free_space",
                harness_align_bottom_right_places_child_at_free_space,
            ),
            (
                "baseline_positions_text_at_offset",
                harness_baseline_positions_text_at_offset,
            ),
            (
                "baseline_dry_baseline_recomputes_per_kind_offsets_after_relayout",
                harness_baseline_dry_baseline_recomputes_per_kind_offsets_after_relayout,
            ),
            (
                "ignore_baseline_hides_its_child_from_a_baseline_row",
                harness_ignore_baseline_hides_its_child_from_a_baseline_row,
            ),
            (
                "fractional_translation_hits_shifted_child_outside_own_bounds",
                harness_fractional_translation_hits_shifted_child_outside_own_bounds,
            ),
            (
                "custom_single_child_layout_positions_child_with_delegate",
                harness_custom_single_child_layout_positions_child_with_delegate,
            ),
            (
                "custom_multi_child_layout_positions_children_by_layout_id",
                harness_custom_multi_child_layout_positions_children_by_layout_id,
            ),
        ],
    );
}

#[test]
fn family_content_paint() {
    run_family(
        "content_paint",
        &[
            (
                "colored_box_self_describes_and_paints",
                harness_colored_box_self_describes_and_paints,
            ),
            (
                "render_error_box_fills_bounded_constraints_and_paints",
                harness_render_error_box_fills_bounded_constraints_and_paints,
            ),
            (
                "render_error_box_falls_back_to_a_finite_extent_on_an_unbounded_axis",
                harness_render_error_box_falls_back_to_a_finite_extent_on_an_unbounded_axis,
            ),
            (
                "custom_paint_orders_background_child_foreground",
                harness_custom_paint_orders_background_child_foreground,
            ),
            ("metadata_with_payload", harness_metadata_with_payload),
            (
                "decorated_box_circle_shape_hit_test_misses_the_corner",
                harness_decorated_box_circle_shape_hit_test_misses_the_corner,
            ),
            (
                "decorated_box_paints_background_before_child",
                harness_decorated_box_paints_background_before_child,
            ),
            (
                "container_matches_the_widget_stack_it_collapses",
                harness_container_matches_the_widget_stack_it_collapses,
            ),
            (
                "container_childless_fills_bounded_and_collapses_unbounded",
                harness_container_childless_fills_bounded_and_collapses_unbounded,
            ),
            (
                "container_singular_transform_paints_and_hits_nothing",
                harness_container_singular_transform_paints_and_hits_nothing,
            ),
        ],
    );
}

#[test]
fn family_text_and_image() {
    run_family(
        "text_and_image",
        &[
            (
                "image_paints_placeholder_frame",
                harness_image_paints_placeholder_frame,
            ),
            (
                "paragraph_paints_text_frame",
                harness_paragraph_paints_text_frame,
            ),
            (
                "editable_lays_out_and_paints_collapsed_caret",
                harness_editable_lays_out_and_paints_collapsed_caret,
            ),
            (
                "editable_paints_the_selection_behind_the_glyphs",
                harness_editable_paints_the_selection_behind_the_glyphs,
            ),
            (
                "editable_composing_underline_paints_at_the_exact_multibyte_box",
                harness_editable_composing_underline_paints_at_the_exact_multibyte_box,
            ),
        ],
    );
}

#[test]
fn family_clips_and_effects() {
    run_family(
        "clips_and_effects",
        &[
            ("clip_rect_self_describes", harness_clip_rect_self_describes),
            ("clip_rrect_wraps_child", harness_clip_rrect_wraps_child),
            ("clip_oval_wraps_child", harness_clip_oval_wraps_child),
            ("clip_path_wraps_child", harness_clip_path_wraps_child),
            (
                "shader_mask_paints_with_shader_mask_layer",
                harness_shader_mask_paints_with_shader_mask_layer,
            ),
            (
                "backdrop_filter_paints_with_backdrop_filter_layer",
                harness_backdrop_filter_paints_with_backdrop_filter_layer,
            ),
            (
                "opacity_paints_with_alpha_layer",
                harness_opacity_paints_with_alpha_layer,
            ),
            (
                "animated_opacity_paint_alpha_tracks_controller_value_at_0_partial_255",
                harness_animated_opacity_paint_alpha_tracks_controller_value_at_0_partial_255,
            ),
            (
                "transform_paints_with_transform_layer",
                harness_transform_paints_with_transform_layer,
            ),
            (
                "transform_to_composes_a_whole_chain",
                harness_transform_to_composes_a_whole_chain,
            ),
            (
                "repaint_boundary_splits_layer_tree",
                harness_repaint_boundary_splits_layer_tree,
            ),
            (
                "physical_model_elevation_casts_shadow_before_fill_and_child",
                harness_physical_model_elevation_casts_shadow_before_fill_and_child,
            ),
            (
                "physical_shape_hit_test_triangular_clipper",
                harness_physical_shape_hit_test_triangular_clipper,
            ),
        ],
    );
}

#[test]
fn family_layer_links() {
    run_family(
        "layer_links",
        &[
            (
                "leader_layer_always_pushes_layer_even_with_zero_children",
                harness_leader_layer_always_pushes_layer_even_with_zero_children,
            ),
            (
                "follower_layer_hit_tests_at_resolved_position_across_repaint_boundaries",
                harness_follower_layer_hit_tests_at_resolved_position_across_repaint_boundaries,
            ),
            (
                "subtree_anchor_attach_publishes_the_real_render_id",
                harness_subtree_anchor_attach_publishes_the_real_render_id,
            ),
            (
                "subtree_anchor_detach_preserves_replacement_publication",
                harness_subtree_anchor_detach_preserves_replacement_publication,
            ),
        ],
    );
}

#[test]
fn family_visibility() {
    run_family(
        "visibility",
        &[
            (
                "offstage_hidden_collapses_and_misses_hits",
                harness_offstage_hidden_collapses_and_misses_hits,
            ),
            (
                "visibility_keeps_child_geometry_while_hidden",
                harness_visibility_keeps_child_geometry_while_hidden,
            ),
        ],
    );
}

#[test]
fn family_semantics() {
    run_family(
        "semantics",
        &[
            (
                "offstage_hidden_drops_its_semantics_subtree",
                harness_offstage_hidden_drops_its_semantics_subtree,
            ),
            (
                "semantics_annotations_builds_semantics_node_and_passes_layout",
                harness_semantics_annotations_builds_semantics_node_and_passes_layout,
            ),
            (
                "indexed_semantics_reports_its_index_and_only_republishes_on_change",
                harness_indexed_semantics_reports_its_index_and_only_republishes_on_change,
            ),
            (
                "merge_semantics_collapses_descendant_boundaries",
                harness_merge_semantics_collapses_descendant_boundaries,
            ),
            (
                "exclude_semantics_drops_descendant_content_but_keeps_layout",
                harness_exclude_semantics_drops_descendant_content_but_keeps_layout,
            ),
            (
                "placed_generation_gate_excludes_a_dropped_child_from_semantics",
                harness_placed_generation_gate_excludes_a_dropped_child_from_semantics,
            ),
            (
                "theater_offstage_from_the_first_pass_publishes_no_semantics",
                harness_theater_offstage_from_the_first_pass_publishes_no_semantics,
            ),
        ],
    );
}

#[test]
fn family_pointer() {
    run_family(
        "pointer",
        &[
            (
                "listener_passes_layout_through_and_attaches_handler",
                harness_listener_passes_layout_through_and_attaches_handler,
            ),
            (
                "mouse_region_uses_one_tracker_target_for_hover_enter_and_exit",
                harness_mouse_region_uses_one_tracker_target_for_hover_enter_and_exit,
            ),
            (
                "absorb_pointer_blocks_child_hits",
                harness_absorb_pointer_blocks_child_hits,
            ),
            (
                "ignore_pointer_lets_hits_pass_to_sibling_below",
                harness_ignore_pointer_lets_hits_pass_to_sibling_below,
            ),
        ],
    );
}

#[test]
fn family_multi_child() {
    run_family(
        "multi_child",
        &[
            (
                "flex_row_positions_children_on_main_axis",
                harness_flex_row_positions_children_on_main_axis,
            ),
            (
                "flex_row_rtl_lays_children_out_from_the_right",
                harness_flex_row_rtl_lays_children_out_from_the_right,
            ),
            (
                "stack_positioned_child_layout_and_hit_test",
                harness_stack_positioned_child_layout_and_hit_test,
            ),
            (
                "indexed_stack_sizes_like_stack_but_only_paints_and_hits_selected_child",
                harness_indexed_stack_sizes_like_stack_but_only_paints_and_hits_selected_child,
            ),
            (
                "list_body_vertical_down_stretches_cross_axis_and_hits_children",
                harness_list_body_vertical_down_stretches_cross_axis_and_hits_children,
            ),
            (
                "theater_skips_leading_children_in_layout_paint_and_hit_test",
                harness_theater_skips_leading_children_in_layout_paint_and_hit_test,
            ),
        ],
    );
}

#[test]
fn family_wrap_flow_table() {
    run_family(
        "wrap_flow_table",
        &[
            (
                "render_wrap_wraps_to_second_run",
                harness_render_wrap_wraps_to_second_run,
            ),
            (
                "wrap_horizontal_rtl_packs_its_run_against_the_right_edge",
                harness_wrap_horizontal_rtl_packs_its_run_against_the_right_edge,
            ),
            (
                "flow_paints_children_in_delegate_order_under_per_child_transform_layers",
                harness_flow_paints_children_in_delegate_order_under_per_child_transform_layers,
            ),
            (
                "flow_degenerate_transform_is_never_hit_but_siblings_still_are",
                harness_flow_degenerate_transform_is_never_hit_but_siblings_still_are,
            ),
            (
                "table_grid_lays_out_each_cell_at_its_exact_offset_and_size",
                harness_table_grid_lays_out_each_cell_at_its_exact_offset_and_size,
            ),
            (
                "table_paints_row_decoration_then_children_then_border_in_order",
                harness_table_paints_row_decoration_then_children_then_border_in_order,
            ),
            (
                "table_baseline_alignment_lines_up_cells_on_their_shared_baseline",
                harness_table_baseline_alignment_lines_up_cells_on_their_shared_baseline,
            ),
            (
                "table_intrinsic_height_measures_the_row_then_stretches_every_cell_to_it",
                harness_table_intrinsic_height_measures_the_row_then_stretches_every_cell_to_it,
            ),
        ],
    );
}

#[test]
fn family_animation() {
    run_family(
        "animation",
        &[
            (
                "render_animated_size_interpolates_over_several_frames_not_snap",
                harness_render_animated_size_interpolates_over_several_frames_not_snap,
            ),
            (
                "render_animated_size_retarget_mid_flight_has_no_discontinuous_jump",
                harness_render_animated_size_retarget_mid_flight_has_no_discontinuous_jump,
            ),
        ],
    );
}

#[test]
fn family_sliver_lists() {
    run_family("sliver_lists", &[
        ("sliver_fixed_extent_list_geometry", harness_sliver_fixed_extent_list_geometry),
        ("sliver_list_seeded_residents_laid_out_at_expected_offsets", harness_sliver_list_seeded_residents_laid_out_at_expected_offsets),
        ("sliver_list_anchor_correction_emits_in_both_scroll_directions", harness_sliver_list_anchor_correction_emits_in_both_scroll_directions),
        ("sliver_main_axis_group_composes_scroll_extents_and_places_children", harness_sliver_main_axis_group_composes_scroll_extents_and_places_children),
        ("scrolling_lazy_request_band", harness_snapshot::scrolling_lazy_sliver_request_band_tracks_scroll_position_and_stays_bounded),
    ]);
}

#[test]
fn family_sliver_wrappers() {
    run_family("sliver_wrappers", &[
        ("sliver_padding_insets_geometry", harness_sliver_padding_insets_geometry),
        ("sliver_to_box_adapter_scroll_extent_matches_child", harness_sliver_to_box_adapter_scroll_extent_matches_child),
        ("sliver_fill_viewport_fraction", harness_sliver_fill_viewport_fraction),
        ("sliver_fill_remaining_uses_viewport_remainder", harness_sliver_fill_remaining_uses_viewport_remainder),
        ("sliver_fill_remaining_and_overscroll_fills_viewport", harness_sliver_fill_remaining_and_overscroll_fills_viewport),
        ("sliver_fill_remaining_with_scrollable_reports_full_scroll_extent", harness_sliver_fill_remaining_with_scrollable_reports_full_scroll_extent),
        ("sliver_ignore_pointer_blocks_hits_when_active", harness_sliver_ignore_pointer_blocks_hits_when_active),
        ("sliver_offstage_hidden_reports_zero_geometry", harness_sliver_offstage_hidden_reports_zero_geometry),
        ("sliver_opacity_alpha_zero_emits_no_opacity_layer", harness_sliver_opacity_alpha_zero_emits_no_opacity_layer),
        ("sliver_animated_opacity_paint_alpha_tracks_controller_value_at_0_partial_255", harness_sliver_animated_opacity_paint_alpha_tracks_controller_value_at_0_partial_255),
    ]);
}

#[test]
fn family_viewport() {
    run_family(
        "viewport",
        &[
            (
                "viewport_stacks_two_slivers",
                harness_viewport_stacks_two_slivers,
            ),
            (
                "shrink_wrapping_viewport_sizes_to_sliver_extent_under_unbounded_main_axis",
                harness_shrink_wrapping_viewport_sizes_to_sliver_extent_under_unbounded_main_axis,
            ),
        ],
    );
}

#[test]
fn family_persistent_header() {
    run_family("persistent_header", &[
        ("sliver_persistent_header_scrolling_shrinks_then_scrolls_off", harness_sliver_persistent_header_scrolling_shrinks_then_scrolls_off),
        ("sliver_persistent_header_pinned_stays_at_zero_and_reports_max_scroll_obstruction_extent", harness_sliver_persistent_header_pinned_stays_at_zero_and_reports_max_scroll_obstruction_extent),
        ("sliver_persistent_header_floating_reveals_on_reverse_scroll_and_pointer_scroll_start_direction_permits_reveal", harness_sliver_persistent_header_floating_reveals_on_reverse_scroll_and_pointer_scroll_start_direction_permits_reveal),
        ("sliver_persistent_header_floating_pinned_shares_reveal_sequence_but_clamps_paint_extent_and_stays_pinned", harness_sliver_persistent_header_floating_pinned_shares_reveal_sequence_but_clamps_paint_extent_and_stays_pinned),
    ]);
}

#[test]
fn family_recovery() {
    run_family(
        "recovery",
        &[
            #[cfg(debug_assertions)]
            (
                "custom_paint_unbalanced_save_poisons_the_paint_phase",
                harness_custom_paint_unbalanced_save_poisons_the_paint_phase,
            ),
            (
                "render_sliver_grid_hit_test_keeps_pre_panic_band_after_a_poisoned_relayout",
                harness_render_sliver_grid_hit_test_keeps_pre_panic_band_after_a_poisoned_relayout,
            ),
            (
                "viewport_degraded_pass_does_not_move_the_scroll_position",
                harness_viewport_degraded_pass_does_not_move_the_scroll_position,
            ),
            (
                "viewport_does_not_serve_a_cache_built_by_a_degraded_pass",
                harness_viewport_does_not_serve_a_cache_built_by_a_degraded_pass,
            ),
        ],
    );
}

fn harness_constrained_box_preserves_large_finite_bound() {
    let width = f64::MAX / 8.0;
    let run = RenderTester::mount(
        box_node(RenderConstrainedBox::new(BoxConstraints::tight(Size::new(
            width, 10.0,
        ))))
        .child(box_node(RenderColoredBox::red(20.0, 10.0)).label("child")),
    )
    .with_constraints(BoxConstraints::new(0.0, f64::MAX / 4.0, 0.0, 100.0))
    .run_layout();
    assert_eq!(run.box_geometry(run.root()), Size::new(width, 10.0));
    assert_eq!(run.box_geometry(run.id("child")), Size::new(width, 10.0));
}

fn harness_constrained_box_keeps_hundredth_quantization() {
    let run = RenderTester::mount(
        box_node(RenderConstrainedBox::new(BoxConstraints::tight(Size::new(
            10.004, 20.006,
        ))))
        .child(box_node(RenderColoredBox::red(20.0, 10.0)).label("child")),
    )
    .with_constraints(loose(100.0))
    .run_layout();
    assert_eq!(run.box_geometry(run.root()), Size::new(10.0, 20.01));
    assert_eq!(run.box_geometry(run.id("child")), Size::new(10.0, 20.01));
}

fn harness_intrinsic_width_preserves_size_with_tiny_step() {
    let mut run = RenderTester::mount(
        box_node(RenderIntrinsicWidth::new(Some(f64::MIN_POSITIVE), None))
            .child(box_node(RenderTestBox::new(10.0, 40.0, 20.0, 80.0)).label("child")),
    )
    .with_constraints(BoxConstraints::new(0.0, 100.0, 0.0, 100.0))
    .run_layout();
    let root = run.root();
    assert_eq!(run.box_geometry(root).width, 40.0);
    assert_eq!(run.box_geometry(run.id("child")).width, 40.0);
    assert_eq!(run.max_intrinsic_width(root, 100.0), 40.0);
}

fn harness_intrinsic_width_rounds_up_to_ordinary_step() {
    let mut run = RenderTester::mount(
        box_node(RenderIntrinsicWidth::new(Some(30.0), None))
            .child(box_node(RenderTestBox::new(10.0, 40.0, 20.0, 80.0)).label("child")),
    )
    .with_constraints(BoxConstraints::new(0.0, 100.0, 0.0, 100.0))
    .run_layout();
    let root = run.root();
    assert_eq!(run.box_geometry(root).width, 60.0);
    assert_eq!(run.box_geometry(run.id("child")).width, 60.0);
    assert_eq!(run.max_intrinsic_width(root, 100.0), 60.0);
}
