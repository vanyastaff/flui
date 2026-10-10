//! The UI runtime's text context, lent through a pipeline's layout, intrinsic and
//! dry queries (ADR-0092 §10 step 3), and released on every failure path.

use flui_foundation::Leaf;
use flui_foundation::geometry::{Matrix4, Size};
use flui_objects::{RenderFlex, RenderParagraph};
use flui_painting::testing::text_context_lends;
use flui_painting::typography::{TextDirection, TextSpan};
use flui_rendering::{
    PipelineOwner, TextContextHandle,
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext, FragmentRecorder},
    error::{PoisonPhase, RenderError},
    parent_data::BoxParentData,
    protocol::{BoxProtocol, Protocol, ProtocolPosition},
    storage::IntrinsicDimension,
    testing::{box_node, inspect, tree},
    traits::{HitTestOutcome, RenderBox, RenderObject, TextBaseline},
};

fn ui_runtime_text() -> TextContextHandle {
    TextContextHandle::standalone()
}

fn lends(text: &TextContextHandle) -> u64 {
    text.with(|text| text_context_lends(text))
}

fn loose() -> BoxConstraints {
    BoxConstraints::new(0.0, 400.0, 0.0, 400.0)
}

/// A leaf that takes the UI runtime's text context and panics while it holds it.
#[derive(Debug)]
struct PanicsWithTheTextContextLent;

impl flui_foundation::Diagnosticable for PanicsWithTheTextContextLent {}

impl RenderBox for PanicsWithTheTextContextLent {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let _lent = ctx.text()?;
        panic!("layout panics while the text context is lent");
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
        false
    }
}

/// Mounts `spec` on `owner`, with loose root constraints.
fn mount_on(owner: &mut PipelineOwner, spec: tree::TreeNode) -> tree::RenderLabelRegistry {
    let (root, labels) = tree::mount(owner, spec);
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(loose()));
    labels
}

/// Mounts `spec` on a fresh pipeline built with `text`, with loose root
/// constraints.
fn mount(
    text: &TextContextHandle,
    spec: tree::TreeNode,
) -> (PipelineOwner, tree::RenderLabelRegistry) {
    let mut owner = PipelineOwner::new(text.clone());
    let labels = mount_on(&mut owner, spec);
    (owner, labels)
}

fn paragraph(text: &str) -> tree::TreeNode {
    box_node(RenderParagraph::new(
        TextSpan::new(text),
        TextDirection::Ltr,
    ))
    .label("paragraph")
}

/// A direct raw implementor receives one complete query capability.
#[derive(Debug)]
struct RawTextQueries;

impl flui_foundation::Diagnosticable for RawTextQueries {}

impl RenderObject<BoxProtocol> for RawTextQueries {
    fn perform_layout_raw(
        &mut self,
        _ctx: &mut <BoxProtocol as Protocol>::LayoutCtxErased<'_>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok(Size::ZERO)
    }

    fn paint_raw(&self, _recorder: &mut FragmentRecorder, _child_count: usize, _size: Size) {}

    fn hit_test_raw(
        &self,
        _position: ProtocolPosition<BoxProtocol>,
        _child_count: usize,
        _size: Size,
        _hit_child: &mut dyn FnMut(
            usize,
            Option<ProtocolPosition<BoxProtocol>>,
            Option<Matrix4>,
        ) -> bool,
    ) -> HitTestOutcome {
        HitTestOutcome::miss()
    }

    fn intrinsic_raw(
        &self,
        dimension: IntrinsicDimension,
        extent: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        ctx.child_intrinsic(0, dimension, extent)
    }

    fn dry_layout_raw(
        &self,
        constraints: BoxConstraints,
        ctx: &mut flui_rendering::context::BoxDryLayoutCtx<'_>,
    ) -> flui_rendering::RenderResult<Size> {
        ctx.child_dry_layout(0, constraints)
    }

    fn dry_baseline_raw(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut flui_rendering::context::BoxDryBaselineCtx<'_>,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        ctx.child_dry_baseline(0, constraints, baseline)
    }
}

fn shared_text_alias_refuses_raw_queries_and_recovers() {
    let text = ui_runtime_text();
    let (mut owner, labels) = mount(
        &text,
        box_node(RawTextQueries).child(paragraph("the child measures real text")),
    );
    let root = owner.root_id().expect("mounted raw parent");
    let child = labels.get("paragraph").expect("mounted paragraph child");
    text.with(|_| {
        for _ in 0..4 {
            assert!(matches!(
                owner.box_intrinsic_dimension(root, IntrinsicDimension::MaxWidth, f64::INFINITY),
                Err(RenderError::TextContextBusy)
            ));
            assert!(matches!(
                owner.box_dry_layout(root, loose()),
                Err(RenderError::TextContextBusy)
            ));
            assert!(matches!(
                owner.box_dry_baseline(root, loose(), TextBaseline::Alphabetic),
                Err(RenderError::TextContextBusy)
            ));
            assert!(!owner.is_layout_poisoned(root));
            assert!(!owner.is_layout_poisoned(child));
        }
    });
    assert_eq!(text.try_with(|_| ()), Some(()), "outer loan has ended");
    assert!(!owner.is_layout_poisoned(root));
    assert!(!owner.is_layout_poisoned(child));

    let width = owner
        .box_intrinsic_dimension(root, IntrinsicDimension::MaxWidth, f64::INFINITY)
        .expect("the same raw parent and child recover after the competing loan ends");
    assert!(
        width.is_finite() && width > 0.0,
        "actual paragraph width: {width}"
    );
    let size = owner
        .box_dry_layout(root, loose())
        .expect("dry layout recovers");
    assert!(size.width.is_finite() && size.width > 0.0);
    assert!(size.height.is_finite() && size.height > 0.0);
    let baseline = owner
        .box_dry_baseline(root, loose(), TextBaseline::Alphabetic)
        .expect("dry baseline recovers")
        .expect("actual paragraph baseline");
    assert!(baseline.is_finite() && baseline > 0.0);
}

fn text_handle_debug_allows_formatter_measurement_reentry() {
    struct MeasuringWriter<'a> {
        text: &'a TextContextHandle,
        painter: flui_painting::TextPainter,
    }

    impl std::fmt::Write for MeasuringWriter<'_> {
        fn write_str(&mut self, _value: &str) -> std::fmt::Result {
            let measured = self.text.try_with(|context| {
                flui_painting::TextMeasurement::new(context, &flui_painting::TextSizing::fixed())
                    .layout(&mut self.painter, 0.0, 400.0)
                    .expect("formatter shapes actual text");
            });
            assert!(
                measured.is_some(),
                "handle Debug must release its own resource borrow before formatter code"
            );
            Ok(())
        }
    }

    let text = ui_runtime_text();
    let mut writer = MeasuringWriter {
        text: &text,
        painter: flui_painting::TextPainter::new()
            .with_text(TextSpan::new("formatter measurement reentry"))
            .with_text_direction(TextDirection::Ltr),
    };
    std::fmt::write(&mut writer, format_args!("{text:?}")).expect("formatting completes");
    let size = writer.painter.size();
    assert!(size.width.is_finite() && size.width > 0.0);
    assert!(size.height.is_finite() && size.height > 0.0);
}

/// A panic unwinds out of a layout that holds the UI runtime's context. The loan
/// ends with the unwind: the sibling paragraph laid out later in the same
/// walk measures through the same context, and so does the next frame. The
/// panic stays the one failure: the panicking leaf is poisoned, the
/// paragraph is not, and a lone panicking root reports `Poisoned` from
/// layout, not a borrow failure.
#[test]
fn a_layout_that_panics_while_holding_the_text_context_releases_it() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let text = ui_runtime_text();
    let (owner, labels) = mount(
        &text,
        box_node(RenderFlex::column())
            .child(box_node(PanicsWithTheTextContextLent).label("panics"))
            .child(
                box_node(RenderParagraph::new(
                    TextSpan::new("after the panic"),
                    TextDirection::Ltr,
                ))
                .label("paragraph"),
            ),
    );
    let panics = labels.get("panics").expect("labelled");
    let paragraph = labels.get("paragraph").expect("labelled");

    let (mut owner, result) = owner.run_frame();
    result.expect("a child's panic is contained in its parent's walk");
    assert!(
        owner.is_layout_poisoned(panics),
        "the panic poisons its node"
    );
    assert!(
        !owner.is_layout_poisoned(paragraph),
        "the sibling measured without a borrow failure"
    );
    let size = inspect::box_geometry(&owner, paragraph).expect("the paragraph laid out");
    assert!(size.width > 0.0 && size.height > 0.0, "got {size:?}");
    let first_frame = lends(&text);
    assert!(
        first_frame > 0,
        "the paragraph measured through the ui_runtime's context"
    );

    owner.mark_needs_layout(paragraph);
    let (owner, result) = owner.run_frame();
    result.expect("the next frame lays out");
    assert!(!owner.is_layout_poisoned(paragraph));
    assert!(
        lends(&text) > first_frame,
        "the next frame measures through the same context"
    );

    let (owner, _) = mount(&text, box_node(PanicsWithTheTextContextLent));
    let mut owner = owner.into_layout();
    let error = owner
        .run_layout()
        .expect_err("a panicking root fails layout");
    assert!(
        matches!(
            error,
            RenderError::Poisoned {
                phase: PoisonPhase::Layout,
                ..
            }
        ),
        "the panic is the reported failure, got {error:?}"
    );
    assert!(
        text.with(|_| true),
        "the context is free once the failed layout returns"
    );

    std::panic::set_hook(hook);
}

/// Intrinsic, dry-layout and dry-baseline queries lend the pipeline's
/// context too, not only layout. Fails if a query path hands the render
/// object a context of its own.
#[test]
fn intrinsic_and_dry_queries_measure_through_the_pipelines_context() {
    let text = ui_runtime_text();
    let (mut owner, labels) = mount(
        &text,
        box_node(RenderParagraph::new(
            TextSpan::new("measured"),
            TextDirection::Ltr,
        ))
        .label("paragraph"),
    );
    let paragraph = labels.get("paragraph").expect("labelled");

    let before = lends(&text);
    let width = owner
        .box_intrinsic_dimension(paragraph, IntrinsicDimension::MaxWidth, f64::INFINITY)
        .expect("intrinsic width");
    let after_intrinsic = lends(&text);
    let size = owner
        .box_dry_layout(paragraph, loose())
        .expect("dry layout");
    let after_dry_layout = lends(&text);
    let baseline = owner
        .box_dry_baseline(paragraph, loose(), TextBaseline::Alphabetic)
        .expect("dry baseline");
    let after_dry_baseline = lends(&text);

    assert!(width > 0.0 && size.width > 0.0 && baseline.is_some_and(|b| b > 0.0));
    assert!(
        after_intrinsic > before,
        "the intrinsic query measured on the ui_runtime's context"
    );
    assert!(
        after_dry_layout > after_intrinsic,
        "dry layout measured on it"
    );
    assert!(
        after_dry_baseline > after_dry_layout,
        "the dry baseline measured on it"
    );
}

/// An owner moved out of its slot for a typestate transition leaves an empty
/// one behind that measures through the same context: a frame driver that
/// takes the owner (and the slot a transition that unwinds leaves) never
/// measures on a context the pipeline was not built with. Fails if the
/// placeholder is built with any other context.
#[test]
fn a_taken_pipeline_leaves_an_owner_that_measures_through_the_same_context() {
    let text = ui_runtime_text();
    let mut slot = PipelineOwner::new(text.clone());
    let taken = slot.take_idle();
    assert!(
        TextContextHandle::ptr_eq(taken.text_context_for_test(), &text),
        "the taken owner keeps the pipeline's context"
    );

    let labels = mount_on(&mut slot, paragraph("on the placeholder"));
    let paragraph = labels.get("paragraph").expect("labelled");
    let before = lends(&text);
    let (slot, result) = slot.run_frame();
    result.expect("the placeholder lays out");
    let size = inspect::box_geometry(&slot, paragraph).expect("the paragraph laid out");
    assert!(size.width > 0.0 && size.height > 0.0, "got {size:?}");
    assert!(
        lends(&text) > before,
        "the placeholder measured through the context the pipeline was built with"
    );
}

// ============================================================================
// A font collection change re-lays out what measured text
// ============================================================================

/// "FLUI Probe Mono" Thin, which no pipeline here names: registering it only
/// changes the collection's generation.
const PROBE_MONO: &[u8] = include_bytes!("../../flui-painting/assets/fonts/probe-mono-100.ttf");

/// A leaf that measures through the UI runtime's context and nothing else: no
/// hook, no painter, no font listener of its own.
#[derive(Debug)]
struct MeasuresThroughTheContext;

impl flui_foundation::Diagnosticable for MeasuresThroughTheContext {}

impl RenderBox for MeasuresThroughTheContext {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        Ok({
            let _lent = ctx.text()?;
            Size::new(10.0, 10.0)
        })
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, BoxParentData>) -> bool {
        false
    }
}

/// Registers the probe face on the collection `text` was built over.
fn register_probe(text: &TextContextHandle) {
    text.with(|text| text.fonts().register_font(PROBE_MONO))
        .expect("the probe face loads");
}

fn needs_layout(owner: &PipelineOwner, id: flui_foundation::RenderId) -> bool {
    owner
        .render_tree()
        .get(id)
        .expect("the node is live")
        .needs_layout()
}

fn needs_paint(owner: &PipelineOwner, id: flui_foundation::RenderId) -> bool {
    owner
        .render_tree()
        .get(id)
        .expect("the node is live")
        .needs_paint()
}

fn rejected_text_scale_recovers(scale: f64) {
    let text = ui_runtime_text();
    let (owner, labels) = mount(
        &text,
        box_node(flui_objects::RenderPadding::all(5.0)).child(paragraph("measured")),
    );
    let paragraph = labels.get("paragraph").expect("labelled");
    let root = owner.root_id().expect("mounted root");
    let (mut owner, result) = owner.run_frame();
    result.expect("the initial text frame succeeds");
    let committed = inspect::box_geometry(&owner, root).expect("committed root size");

    let impact = flui_rendering::testing::edit_render_object::<RenderParagraph, _, _>(
        &mut owner,
        paragraph,
        |paragraph| paragraph.set_text_scale_factor(scale),
    );
    owner.apply_render_update_impact(paragraph, impact);

    let queries = [
        owner
            .box_intrinsic_dimension(root, IntrinsicDimension::MaxWidth, f64::INFINITY)
            .map(|_| ()),
        owner.box_dry_layout(root, loose()).map(|_| ()),
        owner
            .box_dry_baseline(root, loose(), TextBaseline::Alphabetic)
            .map(|_| ()),
    ];
    for query in queries {
        assert!(
            matches!(query, Err(RenderError::TextLayout(_))),
            "query: {query:?}"
        );
    }

    for attempt in 0..4 {
        let (next, result) = owner.run_frame();
        owner = next;
        assert!(
            matches!(result, Err(RenderError::TextLayout(_))),
            "attempt {attempt}: {result:?}"
        );
        assert!(
            !owner.is_layout_poisoned(root),
            "authored input must not poison the root"
        );
        assert!(
            !owner.is_layout_poisoned(paragraph),
            "authored input must not poison its consumer"
        );
        assert_eq!(inspect::box_geometry(&owner, root), Some(committed));
    }

    let impact = flui_rendering::testing::edit_render_object::<RenderParagraph, _, _>(
        &mut owner,
        paragraph,
        |paragraph| paragraph.set_text_scale_factor(1.0),
    );
    owner.apply_render_update_impact(paragraph, impact);
    let (owner, result) = owner.run_frame();
    result.expect("corrected text succeeds in the existing tree");
    assert_eq!(inspect::box_geometry(&owner, root), Some(committed));
    assert!(!owner.has_dirty_nodes(), "recovery consumes retained work");
}

fn zero_text_scale_recovers() {
    rejected_text_scale_recovers(0.0);
}

fn non_finite_text_scale_recovers() {
    rejected_text_scale_recovers(f64::NAN);
}

fn overflowing_text_scale_recovers() {
    rejected_text_scale_recovers(f64::MAX);
}

fn rejected_viewport_text_preserves_committed_metrics(shrink_wrap: bool) {
    use flui_objects::{RenderShrinkWrappingViewport, RenderSliverToBoxAdapter, RenderViewport};
    use flui_rendering::{constraints::AxisDirection, testing::sliver_node};

    let child = sliver_node(RenderSliverToBoxAdapter::new()).child(paragraph("measured"));
    let viewport = if shrink_wrap {
        box_node(RenderShrinkWrappingViewport::new(
            AxisDirection::TopToBottom,
        ))
        .child(child)
    } else {
        box_node(RenderViewport::new(AxisDirection::TopToBottom)).child(child)
    };
    let (owner, labels) = mount(&ui_runtime_text(), viewport);
    let paragraph = labels.get("paragraph").expect("labelled paragraph");
    let root = owner.root_id().expect("mounted viewport");
    let (mut owner, result) = owner.run_frame();
    result.expect("valid viewport text lays out");
    let metrics = |owner: &PipelineOwner| {
        let node = owner.render_tree().get(root).expect("live viewport");
        if shrink_wrap {
            let viewport = node
                .downcast_render_object::<RenderShrinkWrappingViewport>()
                .expect("shrink-wrapping viewport");
            (viewport.max_scroll_extent(), viewport.has_visual_overflow())
        } else {
            let viewport = node
                .downcast_render_object::<RenderViewport>()
                .expect("viewport");
            (viewport.max_scroll_extent(), viewport.has_visual_overflow())
        }
    };
    let committed = metrics(&owner);
    assert!(committed.0 > 0.0, "real text contributes a scroll extent");
    let impact = flui_rendering::testing::edit_render_object::<RenderParagraph, _, _>(
        &mut owner,
        paragraph,
        |paragraph| paragraph.set_text_scale_factor(f64::MAX),
    );
    owner.apply_render_update_impact(paragraph, impact);
    let (mut owner, result) = owner.run_frame();
    assert!(matches!(result, Err(RenderError::TextLayout(_))));
    assert_eq!(
        metrics(&owner),
        committed,
        "a rejected pass publishes no metrics"
    );
    let impact = flui_rendering::testing::edit_render_object::<RenderParagraph, _, _>(
        &mut owner,
        paragraph,
        |paragraph| paragraph.set_text_scale_factor(1.0),
    );
    owner.apply_render_update_impact(paragraph, impact);
    let (owner, result) = owner.run_frame();
    result.expect("corrected text recovers in the mounted viewport");
    assert_eq!(metrics(&owner), committed);
}

fn rejected_viewport_text_preserves_metrics() {
    rejected_viewport_text_preserves_committed_metrics(false);
}

fn rejected_shrink_wrapping_viewport_text_preserves_metrics() {
    rejected_viewport_text_preserves_committed_metrics(true);
}

fn rejected_root_text_preserves_the_published_view_size() {
    use flui_rendering::view::{RenderView, RenderViewAdapter};

    let (owner, labels) = mount(
        &ui_runtime_text(),
        box_node(RenderViewAdapter::new(RenderView::new())).child(paragraph("measured")),
    );
    let root = owner.root_id().expect("mounted view");
    let paragraph = labels.get("paragraph").expect("labelled paragraph");
    let (mut owner, result) = owner.run_frame();
    result.expect("initial view lays out");
    let published_size = |owner: &PipelineOwner| {
        owner
            .render_tree()
            .get(root)
            .expect("live view")
            .downcast_render_object::<RenderViewAdapter>()
            .expect("view adapter")
            .view
            .size()
    };
    let committed = published_size(&owner);
    let resized = Size::new(600.0, 500.0);
    owner.set_root_constraints(Some(BoxConstraints::tight(resized)));
    let impact = flui_rendering::testing::edit_render_object::<RenderParagraph, _, _>(
        &mut owner,
        paragraph,
        |paragraph| paragraph.set_text_scale_factor(f64::MAX),
    );
    owner.apply_render_update_impact(paragraph, impact);
    let (mut owner, result) = owner.run_frame();
    assert!(matches!(result, Err(RenderError::TextLayout(_))));
    assert_eq!(published_size(&owner), committed);
    assert_eq!(inspect::box_geometry(&owner, root), Some(committed));
    let impact = flui_rendering::testing::edit_render_object::<RenderParagraph, _, _>(
        &mut owner,
        paragraph,
        |paragraph| paragraph.set_text_scale_factor(1.0),
    );
    owner.apply_render_update_impact(paragraph, impact);
    let (owner, result) = owner.run_frame();
    result.expect("corrected text completes the pending resize");
    assert_eq!(published_size(&owner), resized);
    assert_eq!(inspect::box_geometry(&owner, root), Some(resized));
}

fn page_resize_survives_text_failure(resize_cross_axis: bool) {
    use flui_objects::{RenderConstrainedBox, RenderSliverToBoxAdapter, RenderViewport};
    use flui_rendering::{
        constraints::AxisDirection,
        testing::sliver_node,
        view::{DimensionChangePolicy, ScrollPosition, ViewportOffset},
    };

    let position = ScrollPosition::new(100.0);
    position.set_dimension_policy(DimensionChangePolicy::KeepFractionalPage {
        viewport_fraction: 1.0,
        initial_page: None,
    });
    let (owner, labels) = mount(
        &ui_runtime_text(),
        box_node(RenderViewport::with_offset(
            AxisDirection::TopToBottom,
            AxisDirection::LeftToRight,
            position.clone(),
        ))
        .child(
            sliver_node(RenderSliverToBoxAdapter::new()).child(
                box_node(RenderConstrainedBox::new(BoxConstraints::new(
                    0.0,
                    f64::INFINITY,
                    1000.0,
                    1000.0,
                )))
                .child(paragraph("measured")),
            ),
        ),
    );
    let paragraph = labels.get("paragraph").expect("labelled paragraph");
    let (mut owner, result) = owner.run_frame();
    result.expect("initial page lays out");
    let committed = position.extents_snapshot();
    assert_eq!(committed.pixels, 100.0);
    assert_eq!(committed.viewport_dimension, 400.0);
    let width = if resize_cross_axis { 500.0 } else { 400.0 };
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(width, 600.0))));
    let impact = flui_rendering::testing::edit_render_object::<RenderParagraph, _, _>(
        &mut owner,
        paragraph,
        |paragraph| paragraph.set_text_scale_factor(f64::MAX),
    );
    owner.apply_render_update_impact(paragraph, impact);
    let (mut owner, result) = owner.run_frame();
    assert!(matches!(result, Err(RenderError::TextLayout(_))));
    assert_eq!(position.viewport_dimension(), 600.0);
    assert_eq!(
        position.pixels(),
        150.0,
        "the accepted resize keeps the fractional page"
    );
    assert_eq!(
        position.max_scroll_extent(),
        if resize_cross_axis {
            committed.max_scroll_extent
        } else {
            400.0
        },
        "only a successful viewport layout publishes content dimensions"
    );
    let impact = flui_rendering::testing::edit_render_object::<RenderParagraph, _, _>(
        &mut owner,
        paragraph,
        |paragraph| paragraph.set_text_scale_factor(1.0),
    );
    owner.apply_render_update_impact(paragraph, impact);
    let (_, result) = owner.run_frame();
    result.expect("valid text completes the pending page resize");
    assert_eq!(position.viewport_dimension(), 600.0);
    assert_eq!(
        position.pixels(),
        150.0,
        "the fractional page survives recovery"
    );
}

fn independent_viewport_resize_survives_text_failure() {
    page_resize_survives_text_failure(false);
}

fn rejected_viewport_resize_preserves_the_fractional_page() {
    page_resize_survives_text_failure(true);
}

fn rejected_tight_text_layout_does_not_stop_the_size_controller() {
    use flui_animation::{Animation as _, AnimationController, Curves, curve::ArcCurve};
    use flui_objects::RenderAnimatedSize;
    use flui_painting::{Alignment, paint::Clip};

    let controller = AnimationController::builder(std::time::Duration::from_secs(1)).build();
    let (owner, labels) = mount(
        &ui_runtime_text(),
        box_node(RenderAnimatedSize::new(
            controller.clone(),
            ArcCurve::new(Curves::Linear),
            Alignment::CENTER,
            Clip::HardEdge,
        ))
        .child(paragraph("measured")),
    );
    let paragraph = labels.get("paragraph").expect("labelled paragraph");
    let root = owner.root_id().expect("mounted size animation");
    let (mut owner, result) = owner.run_frame();
    result.expect("initial text lays out");
    let committed = inspect::box_geometry(&owner, root);
    controller.forward().expect("controller starts");
    controller.tick_at(std::time::Duration::ZERO);
    controller.tick_at(std::time::Duration::from_millis(20));
    let before = controller.value();
    assert!(before > 0.0);
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(400.0, 400.0))));
    let impact = flui_rendering::testing::edit_render_object::<RenderParagraph, _, _>(
        &mut owner,
        paragraph,
        |paragraph| paragraph.set_text_scale_factor(f64::MAX),
    );
    owner.apply_render_update_impact(paragraph, impact);
    let (mut owner, result) = owner.run_frame();
    assert!(matches!(result, Err(RenderError::TextLayout(_))));
    assert_eq!(inspect::box_geometry(&owner, root), committed);
    controller.tick_at(std::time::Duration::from_millis(40));
    assert!(
        controller.value() > before,
        "rejected child layout cannot stop accepted animation"
    );
    let impact = flui_rendering::testing::edit_render_object::<RenderParagraph, _, _>(
        &mut owner,
        paragraph,
        |paragraph| paragraph.set_text_scale_factor(1.0),
    );
    owner.apply_render_update_impact(paragraph, impact);
    let (owner, result) = owner.run_frame();
    result.expect("valid text accepts the tight size");
    assert_eq!(
        inspect::box_geometry(&owner, root),
        Some(Size::new(400.0, 400.0))
    );
}

#[test]
fn rejected_text_layout_is_fallible_through_queries_and_frames() {
    crate::run_table(&[
        (
            "shared_text_alias_refuses_raw_queries_and_recovers",
            shared_text_alias_refuses_raw_queries_and_recovers,
        ),
        (
            "text_handle_debug_allows_formatter_measurement_reentry",
            text_handle_debug_allows_formatter_measurement_reentry,
        ),
        (
            "numeric_sizing_is_presentation_local",
            numeric_sizing_is_presentation_local,
        ),
        (
            "numeric_sizing_commits_every_measurement_before_reentrant_wake",
            numeric_sizing_commits_every_measurement_before_reentrant_wake,
        ),
        (
            "numeric_sizing_retains_wake_capture_across_last_owner_release",
            numeric_sizing_retains_wake_capture_across_last_owner_release,
        ),
        ("zero_text_scale_recovers", zero_text_scale_recovers),
        (
            "non_finite_text_scale_recovers",
            non_finite_text_scale_recovers,
        ),
        (
            "overflowing_text_scale_recovers",
            overflowing_text_scale_recovers,
        ),
        (
            "rejected_viewport_text_preserves_metrics",
            rejected_viewport_text_preserves_metrics,
        ),
        (
            "rejected_shrink_wrapping_viewport_text_preserves_metrics",
            rejected_shrink_wrapping_viewport_text_preserves_metrics,
        ),
        (
            "rejected_root_text_preserves_the_published_view_size",
            rejected_root_text_preserves_the_published_view_size,
        ),
        (
            "independent_viewport_resize_survives_text_failure",
            independent_viewport_resize_survives_text_failure,
        ),
        (
            "rejected_viewport_resize_preserves_the_fractional_page",
            rejected_viewport_resize_preserves_the_fractional_page,
        ),
        (
            "rejected_tight_text_layout_does_not_stop_the_size_controller",
            rejected_tight_text_layout_does_not_stop_the_size_controller,
        ),
        (
            "rejected_viewport_correction_preserves_scroll_metrics",
            rejected_viewport_correction_preserves_scroll_metrics,
        ),
        (
            "rejected_shrink_wrapping_correction_preserves_scroll_metrics",
            rejected_shrink_wrapping_correction_preserves_scroll_metrics,
        ),
        (
            "rejected_shrink_wrapping_correction_preserves_viewport_dimension",
            rejected_shrink_wrapping_correction_preserves_viewport_dimension,
        ),
        (
            "scroll_input_during_layout_survives_rejection",
            scroll_input_during_layout_survives_rejection,
        ),
        (
            "scroll_input_during_shrink_wrapping_layout_survives_rejection",
            scroll_input_during_shrink_wrapping_layout_survives_rejection,
        ),
        (
            "text_failure_retains_independent_pending_layout",
            text_failure_retains_independent_pending_layout,
        ),
        (
            "fixed_offset_refuses_a_stale_proposal",
            fixed_offset_refuses_a_stale_proposal,
        ),
        (
            "scrollable_offset_refuses_a_stale_proposal",
            scrollable_offset_refuses_a_stale_proposal,
        ),
        (
            "scroll_position_refuses_a_stale_proposal",
            scroll_position_refuses_a_stale_proposal,
        ),
        (
            "unchanged_nonfinite_offset_is_not_new_input",
            unchanged_nonfinite_offset_is_not_new_input,
        ),
        (
            "shrink_wrapped_page_resize_measures_the_mapped_position",
            shrink_wrapped_page_resize_measures_the_mapped_position,
        ),
    ]);
}

fn numeric_sizing_commits_every_measurement_before_reentrant_wake() {
    use flui_foundation::RenderId;
    use flui_objects::RenderConstrainedBox;
    use flui_painting::TextSizing;
    use flui_rendering::PipelineCell;
    use std::cell::RefCell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    };

    thread_local! {
        static PUBLICATION_CELL: RefCell<Option<(PipelineCell, [RenderId; 2])>> = const { RefCell::new(None) };
    }

    for fail_wake in [false, true] {
        let text = ui_runtime_text();
        let spec = box_node(RenderFlex::column())
            .child(
                box_node(RenderConstrainedBox::new(BoxConstraints::tight(Size::new(
                    180.0, 50.0,
                ))))
                .child(
                    box_node(RenderParagraph::new(
                        TextSpan::new("first paragraph"),
                        TextDirection::Ltr,
                    ))
                    .label("first"),
                ),
            )
            .child(
                box_node(RenderConstrainedBox::new(BoxConstraints::tight(Size::new(
                    180.0, 50.0,
                ))))
                .child(
                    box_node(RenderParagraph::new(
                        TextSpan::new("second independent paragraph"),
                        TextDirection::Ltr,
                    ))
                    .label("second"),
                ),
            );
        let (owner, labels) = mount(&text, spec);
        let (owner, frame) = owner.run_frame();
        frame.expect("initial paragraphs frame");
        let ids = [
            labels.get("first").expect("first"),
            labels.get("second").expect("second"),
        ];
        let cell = PipelineCell::new(owner);
        let widths = |owner: &mut PipelineOwner| {
            ids.map(|id| {
                owner
                    .box_intrinsic_dimension(id, IntrinsicDimension::MaxWidth, f64::INFINITY)
                    .expect("paragraph intrinsic")
            })
        };
        let initial = cell.with_mut(widths);
        let observed = Arc::new(Mutex::new(None));
        let seen = Arc::clone(&observed);
        let healthy_wakes = Arc::new(AtomicUsize::new(0));
        let successor_wakes = Arc::clone(&healthy_wakes);
        PUBLICATION_CELL.with(|slot| *slot.borrow_mut() = Some((cell.clone(), ids)));
        cell.with_mut(|owner| {
            owner.set_on_need_visual_update(move || {
                let (cell, ids) = PUBLICATION_CELL.with(|slot| {
                    slot.borrow()
                        .as_ref()
                        .expect("owner-local callback target")
                        .clone()
                });
                let widths = cell.with_mut(|owner| {
                    let widths = ids.map(|id| {
                        owner
                            .box_intrinsic_dimension(
                                id,
                                IntrinsicDimension::MaxWidth,
                                f64::INFINITY,
                            )
                            .expect("updated paragraph intrinsic")
                    });
                    let wakes = Arc::clone(&successor_wakes);
                    owner.set_on_need_visual_update(move || {
                        wakes.fetch_add(1, Ordering::SeqCst);
                    });
                    widths
                });
                *seen.lock().expect("observation lock") = Some(widths);
                assert!(!fail_wake, "deferred sizing wake failure");
            });
        });
        let update = cell
            .with_mut(|owner| owner.set_text_sizing(TextSizing::linear(2.0).expect("valid scale")))
            .expect("measurement wake");
        let outcome = catch_unwind(AssertUnwindSafe(|| update.notify()));
        if fail_wake {
            let failure = outcome.expect_err("wake failure remains authoritative");
            assert_eq!(
                failure.downcast_ref::<&str>(),
                Some(&"deferred sizing wake failure")
            );
        } else {
            outcome.expect("healthy reentrant wake");
        }
        let updated = observed
            .lock()
            .expect("observations")
            .expect("wake observed both paragraphs");
        for index in 0..2 {
            assert!(
                updated[index] > initial[index] * 1.9,
                "every taken measurement must be invalidated before waking"
            );
        }
        let update = cell
            .with_mut(|owner| owner.set_text_sizing(TextSizing::linear(3.0).expect("valid scale")))
            .expect("next publication wake");
        update.notify();
        assert_eq!(
            healthy_wakes.load(Ordering::SeqCst),
            1,
            "replacement hook serves the next publication"
        );
        let recovered = cell.with_mut(widths);
        for index in 0..2 {
            assert!(recovered[index] > updated[index] * 1.4);
        }
        PUBLICATION_CELL.with(|slot| slot.borrow_mut().take());
    }
}

fn numeric_sizing_retains_wake_capture_across_last_owner_release() {
    use flui_painting::TextSizing;
    use flui_rendering::{PipelineCell, pipeline::WeakPipelineCell};
    use std::cell::RefCell;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    thread_local! {
        static LAST_OWNER: RefCell<Option<PipelineCell>> = const { RefCell::new(None) };
        static OWNER_OBSERVER: RefCell<Option<WeakPipelineCell>> = const { RefCell::new(None) };
    }

    struct Capture {
        retirements: Arc<AtomicUsize>,
        fail: bool,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            assert!(
                OWNER_OBSERVER.with(|slot| slot
                    .borrow()
                    .as_ref()
                    .expect("owner observer")
                    .upgrade()
                    .is_none()),
                "capture retires only after the physical owner is gone"
            );
            self.retirements.fetch_add(1, Ordering::SeqCst);
            assert!(!self.fail, "deferred capture retirement failure");
        }
    }

    for (invocation_fails, retirement_fails) in
        [(false, false), (false, true), (true, false), (true, true)]
    {
        let text = ui_runtime_text();
        let (mut owner, labels) = mount(&text, paragraph("retained wake capture"));
        let id = labels.get("paragraph").expect("paragraph");
        owner
            .box_intrinsic_dimension(id, IntrinsicDimension::MaxWidth, f64::INFINITY)
            .expect("record real text measurement");
        let cell = PipelineCell::new(owner);
        let retirements = Arc::new(AtomicUsize::new(0));
        let calls = Arc::new(AtomicUsize::new(0));
        let invoked = Arc::clone(&calls);
        let capture = Capture {
            retirements: Arc::clone(&retirements),
            fail: retirement_fails,
        };
        OWNER_OBSERVER.with(|slot| *slot.borrow_mut() = Some(cell.downgrade()));
        LAST_OWNER.with(|slot| *slot.borrow_mut() = Some(cell.clone()));
        cell.with_mut(|owner| {
            owner.set_on_need_visual_update(move || {
                let _capture = &capture;
                invoked.fetch_add(1, Ordering::SeqCst);
                let owner = LAST_OWNER
                    .with(|slot| slot.borrow_mut().take())
                    .expect("last physical owner");
                assert!(owner.is_free(), "publication releases the pipeline loan");
                drop(owner);
                assert!(!invocation_fails, "deferred invocation failure");
            });
        });
        let update = cell
            .with_mut(|owner| owner.set_text_sizing(TextSizing::linear(2.0).expect("valid scale")))
            .expect("staged wake");
        drop(cell);
        assert_eq!(
            retirements.load(Ordering::SeqCst),
            0,
            "pending notification owns the capture"
        );
        let outcome = catch_unwind(AssertUnwindSafe(|| update.notify()));
        if invocation_fails || retirement_fails {
            let failure = outcome.expect_err("first failure propagates after custody settles");
            let expected = if invocation_fails {
                "deferred invocation failure"
            } else {
                "deferred capture retirement failure"
            };
            assert_eq!(failure.downcast_ref::<&str>(), Some(&expected));
        } else {
            outcome.expect("healthy invocation and retirement");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            retirements.load(Ordering::SeqCst),
            usize::from(!invocation_fails),
            "a caught invocation failure retains captures instead of entering a competing destructor"
        );
        assert!(OWNER_OBSERVER.with(|slot| {
            slot.borrow()
                .as_ref()
                .expect("owner observer")
                .upgrade()
                .is_none()
        }));
        OWNER_OBSERVER.with(|slot| slot.borrow_mut().take());

        let (mut replacement, labels) = mount(&text, paragraph("next independent presentation"));
        let id = labels.get("paragraph").expect("replacement paragraph");
        let normal = replacement
            .box_intrinsic_dimension(id, IntrinsicDimension::MaxWidth, f64::INFINITY)
            .expect("replacement measurement");
        let wakes = Arc::new(AtomicUsize::new(0));
        let requested = Arc::clone(&wakes);
        replacement.set_on_need_visual_update(move || {
            requested.fetch_add(1, Ordering::SeqCst);
        });
        replacement
            .set_text_sizing(TextSizing::linear(2.0).expect("valid scale"))
            .expect("replacement wake")
            .notify();
        assert_eq!(wakes.load(Ordering::SeqCst), 1);
        let grown = replacement
            .box_intrinsic_dimension(id, IntrinsicDimension::MaxWidth, f64::INFINITY)
            .expect("healthy replacement measurement");
        assert!(grown > normal * 1.9);
    }
}

fn numeric_sizing_is_presentation_local() {
    use flui_foundation::{TextScaleProfile, TextSize, TextSizeRequest};
    use flui_painting::TextSizing;
    let text = ui_runtime_text();
    let (mut first, first_labels) = mount(&text, paragraph("same authored paragraph"));
    let (mut second, second_labels) = mount(&text, paragraph("same authored paragraph"));
    let a = first_labels.get("paragraph").expect("first paragraph");
    let b = second_labels.get("paragraph").expect("second paragraph");
    let query = |owner: &mut PipelineOwner, id| {
        owner
            .box_intrinsic_dimension(id, IntrinsicDimension::MaxWidth, f64::INFINITY)
            .expect("finite intrinsic")
    };
    let normal = query(&mut first, a);
    assert_eq!(query(&mut second, b), normal);
    let sizing = TextSizing::exact([(
        TextSizeRequest {
            size: TextSize::new(14.0).expect("authored"),
            profile: TextScaleProfile::Body,
        },
        TextSize::new(27.25).expect("answer"),
    )])
    .expect("consistent answers");
    if let Some(update) = first.set_text_sizing(sizing) {
        update.notify();
    }
    let grown = query(&mut first, a);
    assert!(
        grown > normal * 1.8,
        "published exact sizing must alter actual child measurement"
    );
    assert_eq!(
        query(&mut second, b),
        normal,
        "shared resources do not share presentation policy"
    );
    let dry = first.box_dry_layout(a, loose()).expect("dry layout");
    let baseline = first
        .box_dry_baseline(a, loose(), TextBaseline::Alphabetic)
        .expect("dry baseline")
        .expect("baseline");
    let (first, result) = first.run_frame();
    result.expect("exact answers shape the frame");
    let geometry = inspect::box_geometry(&first, a).expect("actual paragraph geometry");
    assert_eq!(geometry, dry);
    assert!(baseline > 20.0);
}

/// Real text measurement fails only on the correction pass, after the first
/// pass discovers that the previous scroll position exceeds the new content.
#[derive(Debug)]
struct CorrectionTextSliver {
    shrink: bool,
    shrunk_extent: f64,
    reject: bool,
    text: TextContextHandle,
    input_during_measurement: Option<flui_rendering::view::ScrollPosition>,
}

impl flui_foundation::Diagnosticable for CorrectionTextSliver {}

impl flui_rendering::traits::RenderSliver for CorrectionTextSliver {
    type Arity = Leaf;
    type ParentData = flui_rendering::parent_data::SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> flui_rendering::RenderResult<flui_rendering::constraints::SliverGeometry> {
        let constraints = *ctx.constraints();
        if let Some(position) = self.input_during_measurement.take() {
            position.set_pixels(200.0);
        }
        let mut painter = flui_painting::TextPainter::new();
        painter.set_text(Some(
            TextSpan::new("measured on the correction pass").into(),
        ));
        painter.set_text_direction(Some(TextDirection::Ltr));
        if self.shrink && self.reject && constraints.scroll_offset < 100.0 {
            painter.set_text_scale_factor(f64::MAX);
        }
        self.text
            .with(|text| painter.layout(text, 0.0, constraints.cross_axis_extent))?;
        let extent: f64 = if self.shrink {
            self.shrunk_extent
        } else {
            1000.0
        };
        let paint_extent =
            (extent - constraints.scroll_offset).clamp(0.0, constraints.remaining_paint_extent);
        Ok(
            flui_rendering::constraints::SliverGeometry::new(extent, paint_extent, 0.0)
                .with_max_paint_extent(extent),
        )
    }
}

fn rejected_correction_preserves_scroll_metrics(shrink_wrap: bool, shrunk_extent: f64) {
    use flui_objects::{RenderShrinkWrappingViewport, RenderViewport};
    use flui_rendering::{
        constraints::AxisDirection,
        testing::sliver_node,
        view::{ScrollPosition, ViewportOffset},
    };

    let position = ScrollPosition::new(100.0);
    let text = ui_runtime_text();
    let child = sliver_node(CorrectionTextSliver {
        shrink: false,
        shrunk_extent,
        reject: true,
        text: text.clone(),
        input_during_measurement: None,
    })
    .label("content");
    let viewport = if shrink_wrap {
        box_node(RenderShrinkWrappingViewport::with_offset(
            AxisDirection::TopToBottom,
            AxisDirection::LeftToRight,
            position.clone(),
        ))
        .child(child)
    } else {
        box_node(RenderViewport::with_offset(
            AxisDirection::TopToBottom,
            AxisDirection::LeftToRight,
            position.clone(),
        ))
        .child(child)
    };
    let (mut owner, labels) = mount(&text, viewport);
    owner.set_root_constraints(Some(BoxConstraints::new(100.0, 100.0, 0.0, 100.0)));
    let content = labels.get("content").expect("labelled content");
    let (mut owner, result) = owner.run_frame();
    result.expect("initial text and scroll metrics are accepted");
    let committed = position.extents_snapshot();
    assert_eq!(committed.max_scroll_extent, 900.0);
    assert_eq!(committed.pixels, 100.0);
    flui_rendering::testing::edit_render_object::<CorrectionTextSliver, _, _>(
        &mut owner,
        content,
        |sliver| sliver.shrink = true,
    );
    owner.mark_needs_layout(content);
    let (mut owner, result) = owner.run_frame();
    assert!(
        matches!(result, Err(RenderError::TextLayout(_))),
        "the second pass must reach rejected text measurement: {result:?}"
    );
    assert_eq!(
        position.extents_snapshot(),
        committed,
        "rejected content dimensions must not reach scroll consumers"
    );
    flui_rendering::testing::edit_render_object::<CorrectionTextSliver, _, _>(
        &mut owner,
        content,
        |sliver| sliver.reject = false,
    );
    owner.mark_needs_layout(content);
    let (_, result) = owner.run_frame();
    result.expect("corrected text completes the pending content change");
    let max = (shrunk_extent - 100.0).max(0.0);
    assert_eq!(position.max_scroll_extent(), max);
    assert_eq!(position.pixels(), max);
    assert_eq!(
        position.viewport_dimension(),
        if shrink_wrap {
            shrunk_extent.min(100.0)
        } else {
            100.0
        }
    );
}

fn rejected_viewport_correction_preserves_scroll_metrics() {
    rejected_correction_preserves_scroll_metrics(false, 150.0);
}

fn rejected_shrink_wrapping_correction_preserves_scroll_metrics() {
    rejected_correction_preserves_scroll_metrics(true, 150.0);
}

fn rejected_shrink_wrapping_correction_preserves_viewport_dimension() {
    rejected_correction_preserves_scroll_metrics(true, 50.0);
}

fn scroll_input_during_layout_is_retried(shrink_wrap: bool) {
    use flui_objects::{RenderShrinkWrappingViewport, RenderViewport};
    use flui_rendering::{
        constraints::AxisDirection,
        testing::sliver_node,
        view::{ScrollPosition, ViewportOffset},
    };

    let text = ui_runtime_text();
    let position = ScrollPosition::new(100.0);
    let child = sliver_node(CorrectionTextSliver {
        shrink: false,
        shrunk_extent: 150.0,
        reject: false,
        text: text.clone(),
        input_during_measurement: None,
    })
    .label("content");
    let viewport = if shrink_wrap {
        box_node(RenderShrinkWrappingViewport::with_offset(
            AxisDirection::TopToBottom,
            AxisDirection::LeftToRight,
            position.clone(),
        ))
        .child(child)
    } else {
        box_node(RenderViewport::with_offset(
            AxisDirection::TopToBottom,
            AxisDirection::LeftToRight,
            position.clone(),
        ))
        .child(child)
    };
    let (mut owner, labels) = mount(&text, viewport);
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(100.0, 100.0))));
    let content = labels.get("content").expect("labelled content");
    let (mut owner, result) = owner.run_frame();
    result.expect("initial content is accepted");
    flui_rendering::testing::edit_render_object::<CorrectionTextSliver, _, _>(
        &mut owner,
        content,
        |sliver| sliver.input_during_measurement = Some(position.clone()),
    );
    owner.mark_needs_layout(content);
    let (owner, result) = owner.run_frame();
    assert!(
        matches!(result, Err(RenderError::ViewportOffsetChanged)),
        "stale layout must be refused: {result:?}"
    );
    assert_eq!(
        position.pixels(),
        200.0,
        "the latest accepted input remains authoritative"
    );
    assert!(
        !owner.layout_waits_for_input(),
        "new input already exists and must remain runnable"
    );
    let (_, result) = owner.run_frame();
    result.expect("the next frame measures against the latest input without another invalidation");
    assert_eq!(position.pixels(), 200.0);
    assert_eq!(position.max_scroll_extent(), 900.0);
}

fn scroll_input_during_layout_survives_rejection() {
    scroll_input_during_layout_is_retried(false);
}

fn scroll_input_during_shrink_wrapping_layout_survives_rejection() {
    scroll_input_during_layout_is_retried(true);
}

fn text_failure_retains_independent_pending_layout() {
    use flui_objects::{RenderConstrainedBox, RenderSliverToBoxAdapter, RenderViewport};
    use flui_rendering::{constraints::AxisDirection, testing::sliver_node, view::ScrollPosition};

    let text = ui_runtime_text();
    let position = ScrollPosition::new(100.0);
    let viewport = |offset| {
        RenderViewport::with_offset(
            AxisDirection::TopToBottom,
            AxisDirection::LeftToRight,
            offset,
        )
    };
    let (owner, labels) = mount(
        &text,
        box_node(RenderFlex::column())
            .child(
                box_node(RenderConstrainedBox::new(BoxConstraints::tight(Size::new(
                    100.0, 100.0,
                ))))
                .child(box_node(viewport(ScrollPosition::new(0.0))).child(
                    sliver_node(RenderSliverToBoxAdapter::new()).child(paragraph("first boundary")),
                )),
            )
            .child(
                box_node(RenderConstrainedBox::new(BoxConstraints::tight(Size::new(
                    100.0, 100.0,
                ))))
                .child(
                    box_node(viewport(position.clone())).child(
                        sliver_node(CorrectionTextSliver {
                            shrink: false,
                            shrunk_extent: 150.0,
                            reject: false,
                            text: text.clone(),
                            input_during_measurement: None,
                        })
                        .label("pending content"),
                    ),
                ),
            ),
    );
    let paragraph = labels.get("paragraph").expect("labelled paragraph");
    let content = labels
        .get("pending content")
        .expect("labelled pending content");
    let (mut owner, result) = owner.run_frame();
    result.expect("both independent boundaries lay out");
    assert_eq!(position.max_scroll_extent(), 900.0);
    let impact = flui_rendering::testing::edit_render_object::<RenderParagraph, _, _>(
        &mut owner,
        paragraph,
        |paragraph| paragraph.set_text_scale_factor(f64::MAX),
    );
    owner.apply_render_update_impact(paragraph, impact);
    flui_rendering::testing::edit_render_object::<CorrectionTextSliver, _, _>(
        &mut owner,
        content,
        |sliver| sliver.shrink = true,
    );
    owner.mark_needs_layout(content);
    assert!(
        owner.nodes_needing_layout().len() >= 2,
        "the failure and pending content must occupy independent queued boundaries"
    );
    let (mut owner, result) = owner.run_frame();
    assert!(matches!(result, Err(RenderError::TextLayout(_))));
    assert_eq!(
        position.max_scroll_extent(),
        900.0,
        "later boundary has not run yet"
    );
    let impact = flui_rendering::testing::edit_render_object::<RenderParagraph, _, _>(
        &mut owner,
        paragraph,
        |paragraph| paragraph.set_text_scale_factor(1.0),
    );
    owner.apply_render_update_impact(paragraph, impact);
    let (_, result) = owner.run_frame();
    result.expect("correcting only the first boundary resumes the accepted tail");
    assert_eq!(
        position.max_scroll_extent(),
        50.0,
        "pending content was laid out without another invalidation"
    );
}

fn stale_proposal_preserves_input<O: flui_rendering::view::ViewportOffset>(mut offset: O) {
    use flui_rendering::view::ViewportLayout as _;

    let mut proposal = offset.begin_layout();
    proposal.correct_by(5.0);
    offset.correct_by(20.0);
    assert!(matches!(
        offset.accept_layout(proposal),
        Err(RenderError::ViewportOffsetChanged)
    ));
    assert_eq!(
        offset.pixels(),
        20.0,
        "stale proposals cannot overwrite accepted input"
    );
    let mut proposal = offset.begin_layout();
    proposal.correct_by(5.0);
    offset
        .accept_layout(proposal)
        .expect("a fresh proposal remains acceptable");
    assert_eq!(offset.pixels(), 25.0);
}

fn fixed_offset_refuses_a_stale_proposal() {
    stale_proposal_preserves_input(flui_rendering::view::FixedViewportOffset::zero());
}

fn scrollable_offset_refuses_a_stale_proposal() {
    stale_proposal_preserves_input(flui_rendering::view::ScrollableViewportOffset::zero());
}

fn scroll_position_refuses_a_stale_proposal() {
    stale_proposal_preserves_input(flui_rendering::view::ScrollPosition::new(0.0));
}

fn unchanged_nonfinite_offset_is_not_new_input() {
    use flui_rendering::view::{
        FixedViewportOffset, ScrollPosition, ScrollableViewportOffset, ViewportOffset,
    };
    fn unchanged<O: ViewportOffset>(mut offset: O) {
        let proposal = offset.begin_layout();
        offset
            .accept_layout(proposal)
            .expect("unchanged admitted input is not a transient change");
    }
    unchanged(FixedViewportOffset::new(f64::NAN));
    unchanged(ScrollableViewportOffset::new(f64::NAN));
    unchanged(ScrollPosition::new(f64::NAN));
}

fn shrink_wrapped_page_resize_measures_the_mapped_position() {
    use flui_objects::{
        RenderConstrainedBox, RenderShrinkWrappingViewport, RenderSliverToBoxAdapter,
    };
    use flui_rendering::{
        constraints::AxisDirection,
        testing::sliver_node,
        view::{DimensionChangePolicy, ScrollPosition, ViewportOffset},
    };

    let position = ScrollPosition::new(100.0);
    position.set_dimension_policy(DimensionChangePolicy::KeepFractionalPage {
        viewport_fraction: 1.0,
        initial_page: None,
    });
    let (mut owner, labels) = mount(
        &ui_runtime_text(),
        box_node(RenderShrinkWrappingViewport::with_offset(
            AxisDirection::TopToBottom,
            AxisDirection::LeftToRight,
            position.clone(),
        ))
        .child(
            sliver_node(RenderSliverToBoxAdapter::new()).child(
                box_node(RenderConstrainedBox::new(BoxConstraints::new(
                    0.0,
                    f64::INFINITY,
                    1000.0,
                    1000.0,
                )))
                .label("page content")
                .child(paragraph("page text")),
            ),
        ),
    );
    let content = labels.get("page content").expect("labelled page content");
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(100.0, 200.0))));
    let (mut owner, result) = owner.run_frame();
    result.expect("initial fractional page is measured");
    assert_eq!(position.pixels(), 100.0);
    assert_eq!(
        inspect::render_offset(&owner, content)
            .expect("measured content")
            .dy,
        -100.0
    );
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(100.0, 400.0))));
    let (owner, result) = owner.run_frame();
    result.expect("resized fractional page is measured");
    assert_eq!(position.pixels(), 200.0);
    assert_eq!(
        inspect::render_offset(&owner, content)
            .expect("resized content")
            .dy,
        -200.0,
        "accepted page mapping and committed child geometry must describe the same position"
    );
}

/// A column of a paragraph and a leaf that measures nothing, laid out once.
fn laid_out_paragraph_and_box(
    text: &TextContextHandle,
) -> (
    PipelineOwner,
    flui_foundation::RenderId,
    flui_foundation::RenderId,
) {
    let (owner, labels) = mount(
        text,
        box_node(RenderFlex::column())
            .child(paragraph("measured"))
            .child(box_node(flui_objects::RenderColoredBox::red(10.0, 10.0)).label("box")),
    );
    let paragraph = labels.get("paragraph").expect("labelled");
    let colored = labels.get("box").expect("labelled");
    let (owner, result) = owner.run_frame();
    result.expect("the first frame lays out");
    assert!(
        !owner.has_dirty_nodes(),
        "the first frame leaves nothing dirty"
    );
    (owner, paragraph, colored)
}

/// A face registered on the collection marks for layout and paint the node
/// that measured text, and not the one that measured none. Fails if the
/// pipeline never learns of the change, or marks the whole tree.
fn a_font_change_marks_only_the_nodes_that_measured_text() {
    let text = ui_runtime_text();
    let (mut owner, paragraph, colored) = laid_out_paragraph_and_box(&text);

    register_probe(&text);
    owner.drain_pending_dirty();

    assert!(
        needs_layout(&owner, paragraph),
        "the paragraph lays out again"
    );
    assert!(needs_paint(&owner, paragraph), "the paragraph repaints");
    assert!(
        !needs_layout(&owner, colored),
        "a node that measured no text is not laid out again"
    );
    let (owner, result) = owner.run_frame();
    result.expect("the frame after the change lays out");
    assert!(!owner.has_dirty_nodes(), "the change is applied once");
}

/// A render object is marked because it measured through its context, with
/// no code of its own that listens for fonts. Fails if the marking needs the
/// render object to opt in.
fn a_render_object_with_no_font_hook_is_marked() {
    let text = ui_runtime_text();
    let (owner, labels) = mount(&text, box_node(MeasuresThroughTheContext).label("leaf"));
    let leaf = labels.get("leaf").expect("labelled");
    let (mut owner, result) = owner.run_frame();
    result.expect("the leaf lays out");

    register_probe(&text);
    owner.drain_pending_dirty();

    assert!(
        needs_layout(&owner, leaf),
        "the leaf that measured is marked"
    );
}

/// Without a registration a drain marks nothing, frame after frame: the
/// record costs no layout while the collection is unchanged. Fails if a
/// drain treats the record itself as a change.
fn no_change_marks_nothing() {
    let text = ui_runtime_text();
    let (mut owner, _, _) = laid_out_paragraph_and_box(&text);

    owner.drain_pending_dirty();
    owner.drain_pending_dirty();

    assert!(!owner.apply_font_change(), "nothing changed");
    assert!(!owner.has_dirty_nodes(), "no drain dirtied a node");
}

/// A drain while the context is lent neither panics nor loses the change: it
/// leaves it for the next drain, which applies it. Fails if the drain borrows
/// the lent context unconditionally (a `BUG:` panic), or if a deferred change
/// is forgotten.
fn a_drain_while_the_context_is_lent_defers_the_change() {
    let text = ui_runtime_text();
    let (mut owner, paragraph, _) = laid_out_paragraph_and_box(&text);

    text.with(|lent| {
        lent.fonts()
            .register_font(PROBE_MONO)
            .expect("the probe face loads");
        owner.drain_pending_dirty();
    });
    assert!(
        !needs_layout(&owner, paragraph),
        "the change waits while the context is lent"
    );

    owner.drain_pending_dirty();
    assert!(
        needs_layout(&owner, paragraph),
        "the next drain lays the paragraph out again"
    );
}

/// A node removed after it measured is skipped, and a survivor that measured
/// is still marked. Fails if a stale record stops the walk or the change
/// reaches no one.
fn a_node_removed_after_measuring_is_skipped() {
    let text = ui_runtime_text();
    let (owner, labels) = mount(
        &text,
        box_node(RenderFlex::column())
            .child(paragraph("removed"))
            .child(
                box_node(RenderParagraph::new(
                    TextSpan::new("kept"),
                    TextDirection::Ltr,
                ))
                .label("kept"),
            ),
    );
    let removed = labels.get("paragraph").expect("labelled");
    let kept = labels.get("kept").expect("labelled");
    let (mut owner, result) = owner.run_frame();
    result.expect("the first frame lays out");
    assert_eq!(owner.remove_render_object(removed), 1);
    let (mut owner, result) = owner.run_frame();
    result.expect("the frame after the removal lays out");

    register_probe(&text);
    owner.drain_pending_dirty();

    assert!(owner.render_tree().get(removed).is_none());
    assert!(
        needs_layout(&owner, kept),
        "the surviving paragraph is marked"
    );
}

/// A long session that builds and drops text nodes and never registers a
/// font keeps the record bounded by the live tree, not by every node it ever
/// built. Fails if removed nodes stay recorded until a font change.
fn the_record_stays_bounded_without_a_font_change() {
    const REBUILDS: usize = 500;
    let text = ui_runtime_text();
    let mut owner = PipelineOwner::new(text);
    for _ in 0..REBUILDS {
        let previous = owner.root_id();
        let labels = mount_on(&mut owner, paragraph("rebuilt"));
        assert!(
            labels.get("paragraph").is_some(),
            "the paragraph is mounted"
        );
        if let Some(previous) = previous {
            assert_eq!(owner.remove_render_object(previous), 1);
        }
        let (next, result) = owner.run_frame();
        result.expect("each rebuild lays out");
        owner = next;
    }
    assert_eq!(owner.render_tree().len(), 1, "one paragraph is live");
    assert!(
        owner.text_measurer_count() <= 65,
        "the record holds {} ids for one live node after {REBUILDS} rebuilds",
        owner.text_measurer_count()
    );
}

/// A face registered on the pipeline's font collection lays out again what
/// measured text through the pipeline's context, and nothing else.
#[test]
fn font_change_contract() {
    let cases: &[(&str, fn())] = &[
        (
            "a_font_change_marks_only_the_nodes_that_measured_text",
            a_font_change_marks_only_the_nodes_that_measured_text,
        ),
        (
            "a_render_object_with_no_font_hook_is_marked",
            a_render_object_with_no_font_hook_is_marked,
        ),
        ("no_change_marks_nothing", no_change_marks_nothing),
        (
            "a_drain_while_the_context_is_lent_defers_the_change",
            a_drain_while_the_context_is_lent_defers_the_change,
        ),
        (
            "a_node_removed_after_measuring_is_skipped",
            a_node_removed_after_measuring_is_skipped,
        ),
        (
            "the_record_stays_bounded_without_a_font_change",
            the_record_stays_bounded_without_a_font_change,
        ),
    ];
    let failed: Vec<&str> = cases
        .iter()
        .filter(|(_, case)| std::panic::catch_unwind(*case).is_err())
        .map(|(name, _)| *name)
        .collect();
    assert!(
        failed.is_empty(),
        "font_change_contract: failing cases: {failed:?}"
    );
}
