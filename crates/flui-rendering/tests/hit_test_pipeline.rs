//! Pipeline hit-test walk — the query twin of the fragment paint walk.
//!
//! Pins the bridge that used to be dead end-to-end (`hit_test_raw`
//! blanket returned `false`, the ctx child recursion was a stub, the
//! registry `RenderView::hit_test` answered `true` with no entries):
//!
//! 1. hits recurse through real children with leaf-first entries;
//! 2. children are tested at their laid-out `RenderState.offset` —
//!    parents no longer mirror offsets in their own fields
//!    (`hit_test_child_at_layout_offset`, Flex's `Vec<Offset>` is gone);
//! 3. a transform parent hit-tests through the INVERSE of its paint
//!    matrix (the D8 hit-test-under-transform gate).

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    hit_testing::HitTestResult,
    objects::{RenderColoredBox, RenderFlex, RenderPadding, RenderTransform},
    parent_data::BoxParentData,
    pipeline::PipelineOwner,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};
use flui_tree::Variable;
use flui_types::{Offset, Size, geometry::px};

type BoxedRenderObject =
    Box<dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>>;

/// Lays out the tree, then returns the laid-out owner for hit queries.
fn laid_out(
    mut owner: PipelineOwner,
    root: flui_foundation::RenderId,
) -> flui_rendering::pipeline::PipelineOwner<flui_rendering::pipeline::phase::Layout> {
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::new(
        px(0.0),
        px(200.0),
        px(0.0),
        px(200.0),
    )));
    let mut owner = owner.into_layout();
    owner.run_layout().expect("layout succeeds");
    owner
}

fn hits(
    owner: &flui_rendering::pipeline::PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    x: f32,
    y: f32,
) -> Vec<flui_foundation::RenderId> {
    let mut result = HitTestResult::new();
    owner.hit_test(Offset::new(px(x), px(y)), &mut result);
    result.path().iter().map(|e| e.target).collect()
}

// ============================================================================
// 1. Leaf-first recursion through a positioned child
// ============================================================================

#[test]
fn padding_child_hits_leaf_first_at_laid_out_offset() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(5.0)) as BoxedRenderObject);
    let child_id = owner
        .insert_child_render_object(padding_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child insert");
    let owner = laid_out(owner, padding_id);

    // (20,20) → child-local (15,15) inside the 40×40 box.
    assert_eq!(
        hits(&owner, 20.0, 20.0),
        vec![child_id, padding_id],
        "hit path must be leaf-first: the colored child, then padding",
    );

    // (3,3) → child-local (-2,-2): inside padding's own area but the
    // padding is hit-transparent (Flutter parity — it forwards to the
    // child only).
    assert!(
        hits(&owner, 3.0, 3.0).is_empty(),
        "padding's own border area claims no hit",
    );
}

// ============================================================================
// 2. Variadic children hit at RenderState offsets (no parent-side Vec)
// ============================================================================

/// Row-like fixture: positions child `i` at `(i*40, 0)` during layout
/// and hit-tests every child at its LAYOUT offset — no parent-side
/// offset mirror anywhere. (RenderFlex itself exercises this same
/// `hit_test_child_at_layout_offset` path, but its FlexParentData hits
/// the production layout walk's BoxParentData limitation — the erased
/// ParentData unit lifts that, and the flex variant of this test lands
/// with it.)
#[derive(Debug)]
struct SimpleRow {
    size: Size,
}

impl flui_foundation::Diagnosticable for SimpleRow {}
impl PaintEffectsCapability for SimpleRow {}
impl SemanticsCapability for SimpleRow {}
impl HotReloadCapability for SimpleRow {}

impl RenderBox for SimpleRow {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) {
        let constraints = *ctx.constraints();
        for i in 0..ctx.child_count() {
            let _ = ctx.layout_child(i, constraints);
            #[allow(clippy::cast_precision_loss)] // test fixture, i < 3
            ctx.position_child(i, Offset::new(px(i as f32 * 40.0), px(0.0)));
        }
        self.size = constraints.constrain(Size::new(px(120.0), px(40.0)));
        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>) -> bool {
        for i in (0..2).rev() {
            if ctx.hit_test_child_at_layout_offset(i) {
                return true;
            }
        }
        false
    }
}

#[test]
fn variadic_children_hit_at_layout_offsets() {
    let mut owner = PipelineOwner::new();
    let row_id = owner.insert(Box::new(SimpleRow { size: Size::ZERO }) as BoxedRenderObject);
    let first = owner
        .insert_child_render_object(row_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child 0");
    let second = owner
        .insert_child_render_object(row_id, Box::new(RenderColoredBox::blue(40.0, 40.0)))
        .expect("child 1");
    let owner = laid_out(owner, row_id);

    let first_hits = hits(&owner, 10.0, 10.0);
    assert_eq!(
        first_hits.first().copied(),
        Some(first),
        "(10,10) lands in the first 40×40 child",
    );

    // The second child sits at the layout offset the row computed
    // (x = 40) — resolved from RenderState.offset by the driver, not
    // from any parent-side offset mirror.
    let second_hits = hits(&owner, 50.0, 10.0);
    assert_eq!(
        second_hits.first().copied(),
        Some(second),
        "(50,10) lands in the second child at its laid-out offset",
    );
}

// ============================================================================
// 3. D8 gate: hit-test under transform walks the inverse paint matrix
// ============================================================================

#[test]
fn transform_child_hits_through_inverse_matrix() {
    let mut owner = PipelineOwner::new();
    let transform_id =
        owner.insert(Box::new(RenderTransform::scale(2.0, 2.0)) as BoxedRenderObject);
    let child_id = owner
        .insert_child_render_object(transform_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child insert");
    let owner = laid_out(owner, transform_id);

    // Visual (50,50) under scale(2,2) came from child-local (25,25) —
    // inside the 40×40 child.
    assert_eq!(
        hits(&owner, 50.0, 50.0),
        vec![child_id, transform_id],
        "the child must receive the inverse-transformed point",
    );

    // Visual (90,90) → child-local (45,45) — outside the child even
    // though it is inside the SCALED visual bounds; without the
    // inverse the naive point would still hit.
    assert!(
        hits(&owner, 90.0, 90.0).is_empty(),
        "outside the inverse-mapped child bounds → miss",
    );
}

// ============================================================================
// 4. RenderFlex itself — FlexParentData through the erased driver
// ============================================================================

/// The production walk's parent-data storage is erased; the typed
/// bridge creates FlexParentData slots lazily. Before that, this exact
/// tree PANICKED in from_erased (the walk hardcoded BoxParentData) —
/// Flex/Stack were impossible in production layout.
#[test]
fn flex_lays_out_and_hits_children_at_layout_offsets() {
    let mut owner = PipelineOwner::new();
    let flex_id = owner.insert(Box::new(RenderFlex::row()) as BoxedRenderObject);
    let first = owner
        .insert_child_render_object(flex_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child 0");
    let second = owner
        .insert_child_render_object(flex_id, Box::new(RenderColoredBox::blue(40.0, 40.0)))
        .expect("child 1");
    let owner = laid_out(owner, flex_id);

    // Layout committed real offsets: the second child sits at x=40.
    let second_offset = owner
        .render_tree()
        .get(second)
        .and_then(|n| n.as_box())
        .map(|e| e.state().offset())
        .expect("child 1 state");
    assert_eq!(
        second_offset,
        Offset::new(px(40.0), px(0.0)),
        "row layout must commit the second child's offset to RenderState",
    );

    assert_eq!(
        hits(&owner, 10.0, 10.0).first().copied(),
        Some(first),
        "(10,10) lands in the first flex child",
    );
    assert_eq!(
        hits(&owner, 50.0, 10.0).first().copied(),
        Some(second),
        "(50,10) lands in the second flex child at its laid-out offset",
    );
}
