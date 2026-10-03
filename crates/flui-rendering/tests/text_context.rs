//! The realm's text context, lent through a pipeline's layout, intrinsic and
//! dry queries (ADR-0092 §10 step 3), and released on every failure path.

use flui_foundation::Leaf;
use flui_foundation::geometry::Size;
use flui_objects::{RenderFlex, RenderParagraph};
use flui_painting::testing::text_context_lends;
use flui_painting::typography::{TextDirection, TextSpan};
use flui_rendering::{
    PipelineOwner, TextContextHandle,
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    error::{PoisonPhase, RenderError},
    parent_data::BoxParentData,
    storage::IntrinsicDimension,
    testing::{box_node, inspect, tree},
    traits::{RenderBox, TextBaseline},
};

fn realm_text() -> TextContextHandle {
    TextContextHandle::standalone()
}

fn lends(text: &TextContextHandle) -> u64 {
    text.with(|text| text_context_lends(text))
}

fn loose() -> BoxConstraints {
    BoxConstraints::new(0.0, 400.0, 0.0, 400.0)
}

/// A leaf that takes the realm's text context and panics while it holds it.
#[derive(Debug)]
struct PanicsWithTheTextContextLent;

impl flui_foundation::Diagnosticable for PanicsWithTheTextContextLent {}

impl RenderBox for PanicsWithTheTextContextLent {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        let _lent = ctx.text();
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

/// A panic unwinds out of a layout that holds the realm's context. The loan
/// ends with the unwind: the sibling paragraph laid out later in the same
/// walk measures through the same context, and so does the next frame. The
/// panic stays the one failure: the panicking leaf is poisoned, the
/// paragraph is not, and a lone panicking root reports `Poisoned` from
/// layout, not a borrow failure.
#[test]
fn a_layout_that_panics_while_holding_the_text_context_releases_it() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let text = realm_text();
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
        "the paragraph measured through the realm's context"
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
    let text = realm_text();
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
        "the intrinsic query measured on the realm's context"
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
    let text = realm_text();
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

/// A leaf that measures through the realm's context and nothing else: no
/// hook, no painter, no font listener of its own.
#[derive(Debug)]
struct MeasuresThroughTheContext;

impl flui_foundation::Diagnosticable for MeasuresThroughTheContext {}

impl RenderBox for MeasuresThroughTheContext {
    type Arity = Leaf;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, BoxParentData>) -> Size {
        let _lent = ctx.text();
        Size::new(10.0, 10.0)
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
    let text = realm_text();
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
    let text = realm_text();
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
    let text = realm_text();
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
    let text = realm_text();
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
    let text = realm_text();
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
    let text = realm_text();
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
