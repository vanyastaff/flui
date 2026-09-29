//! The realm's text context, lent through a pipeline's layout, intrinsic and
//! dry queries (ADR-0092 §10 step 3), and released on every failure path.

use flui_foundation::Leaf;
use flui_foundation::geometry::Size;
use flui_objects::{RenderFlex, RenderParagraph};
use flui_painting::testing::text_context_lends;
use flui_painting::typography::{TextDirection, TextSpan};
use flui_painting::{FontCollection, TextContext};
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
    TextContextHandle::new(TextContext::new(&FontCollection::new()))
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

/// Mounts `spec` on a fresh pipeline that lends `text`, with loose root
/// constraints.
fn mount(
    text: Option<&TextContextHandle>,
    spec: tree::TreeNode,
) -> (PipelineOwner, tree::RenderLabelRegistry) {
    let mut owner = PipelineOwner::new();
    if let Some(text) = text {
        owner.set_text_context(text.clone());
    }
    let (root, labels) = tree::mount(&mut owner, spec);
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(loose()));
    (owner, labels)
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
        Some(&text),
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

    let (owner, _) = mount(Some(&text), box_node(PanicsWithTheTextContextLent));
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
        Some(&text),
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

/// A pipeline the runtime never gave a context to still measures: it builds
/// one of its own on its first layout and keeps it.
#[test]
fn a_pipeline_without_a_handle_measures_on_its_own_context() {
    let (owner, labels) = mount(
        None,
        box_node(RenderParagraph::new(
            TextSpan::new("no realm"),
            TextDirection::Ltr,
        ))
        .label("paragraph"),
    );
    let paragraph = labels.get("paragraph").expect("labelled");
    assert!(owner.text_context_for_test().is_none());

    let (owner, result) = owner.run_frame();
    result.expect("layout succeeds");
    let size = inspect::box_geometry(&owner, paragraph).expect("the paragraph laid out");
    assert!(size.width > 0.0 && size.height > 0.0, "got {size:?}");
    let own = owner
        .text_context_for_test()
        .expect("the pipeline built a context of its own")
        .clone();
    assert!(
        lends(&own) > 0,
        "the paragraph measured on the pipeline's own context"
    );
}
