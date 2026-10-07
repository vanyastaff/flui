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
//!    matrix; child descent records paint offsets on the result
//!    transform stack for gesture dispatch.

use flui_foundation::geometry::Offset;
use flui_objects::{RenderColoredBox, RenderFlex, RenderPadding, RenderTransform};
use flui_rendering::{pipeline::PipelineOwner, testing::inspect};

use crate::common::{BoxedRenderObject, laid_out_loose_200x200 as laid_out};

fn hits(
    owner: &flui_rendering::pipeline::PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    x: f64,
    y: f64,
) -> Vec<flui_foundation::RenderId> {
    inspect::hit_path(owner, x, y)
}

// ============================================================================
// 1. Leaf-first recursion through a positioned child
// ============================================================================

pub(crate) fn padding_child_hits_leaf_first_at_laid_out_offset() {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
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
    // padding is hit-transparent (it forwards to the child only).
    assert!(
        hits(&owner, 3.0, 3.0).is_empty(),
        "padding's own border area claims no hit",
    );
}

// ============================================================================
// 2. Variadic children hit at RenderState offsets (no parent-side Vec)
// ============================================================================

// ============================================================================
// 3. D8 gate: hit-test under transform walks the inverse paint matrix
// ============================================================================

pub(crate) fn transform_child_hits_through_inverse_matrix() {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
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

pub(crate) fn perspective_transform_unprojects_to_the_child_plane() {
    use flui_foundation::geometry::{Matrix4, Point};

    // A Y rotation with cos=0.6, sin=0.8, followed by w=1+z/10.
    // On the child plane: screen=(0.6*x, y)/(1-0.08*x).
    let transform = Matrix4::from([
        0.6, 0.0, -0.8, -0.08, 0.0, 1.0, 0.0, 0.0,
        0.8, 0.0, 0.6, 0.06, 0.0, 0.0, 0.0, 1.0,
    ]);
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let parent = owner.insert(Box::new(RenderTransform::new(transform)) as BoxedRenderObject);
    let child = owner.insert_child_render_object(parent,
        Box::new(RenderColoredBox::red(1.5, 10.0))).expect("child insert");
    let owner = laid_out(owner, parent);

    // Local (2,3) is outside the narrow child; inverse(screen_x,screen_y,0)
    // incorrectly gives x=6/7 and therefore admits it.
    let screen = Point::new(10.0 / 7.0, 25.0 / 7.0);
    let local = owner.global_to_local(child, screen, Some(parent)).expect("visible plane");
    assert!((local.x - 2.0).abs() < 1e-10 && (local.y - 3.0).abs() < 1e-10,
        "unprojection must recover the actual child-plane point: {local:?}");
    assert!(hits(&owner, screen.x, screen.y).is_empty(), "the actual point misses the child");
    // Local (1,2) remains hittable.
    assert_eq!(hits(&owner, 0.6 / 0.92, 2.0 / 0.92).first().copied(), Some(child));
}

pub(crate) fn perspective_transform_refuses_hidden_and_degenerate_planes() {
    use flui_foundation::geometry::{Matrix4, Point};

    for (name, transform, screen) in [
        ("behind camera", Matrix4::from([
            -1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, -1.0,
        ]), Point::new(2.0, 3.0)),
        ("horizon", Matrix4::from([
            1.0, 0.0, 0.0, -0.5, 0.0, 1.0, 0.0, 0.0,
            0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]), Point::new(-2.0, 3.0)),
        ("edge-on plane", Matrix4::from([
            0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0,
            1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ]), Point::new(2.0, 3.0)),
    ] {
        let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
        let parent = owner.insert(Box::new(RenderTransform::new(transform)) as BoxedRenderObject);
        let child = owner.insert_child_render_object(parent,
            Box::new(RenderColoredBox::red(10.0, 10.0))).expect("child insert");
        let owner = laid_out(owner, parent);
        assert_eq!(owner.global_to_local(child, screen, Some(parent)), None, "{name}");
        assert!(hits(&owner, screen.x, screen.y).is_empty(), "{name} must not hit a child");
    }
}

// ============================================================================
// 4. RenderFlex itself — FlexParentData through the erased driver
// ============================================================================

/// The production walk's parent-data storage is erased; the typed
/// bridge creates FlexParentData slots lazily. Before that, this exact
/// tree PANICKED in from_erased (the walk hardcoded BoxParentData) —
/// Flex/Stack were impossible in production layout.
pub(crate) fn flex_lays_out_and_hits_children_at_layout_offsets() {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
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
        Offset::new(40.0, 0.0),
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

// ============================================================================
// 5. Sliver subtree hit-testing through a Box host
// ============================================================================

#[derive(Clone, Copy, Debug)]
enum FailedHitScope {
    Offset,
    Transform,
    Nested,
    ChildOffset,
    ChildLayoutOffset,
    ChildOverride,
}

#[derive(Debug)]
struct RecoveringHitParent(FailedHitScope);

impl flui_foundation::Diagnosticable for RecoveringHitParent {}

impl flui_rendering::traits::RenderBox for RecoveringHitParent {
    type Arity = flui_foundation::Variable;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_foundation::geometry::Size {
        let first_offset = if matches!(self.0, FailedHitScope::ChildOverride) {
            Offset::ZERO
        } else {
            Offset::new(20.0, 0.0)
        };
        for (index, offset) in [first_offset, Offset::new(5.0, 0.0)]
            .into_iter()
            .enumerate()
        {
            ctx.layout_child(
                index,
                flui_rendering::constraints::BoxConstraints::tight(
                    flui_foundation::geometry::Size::new(40.0, 40.0),
                ),
            );
            ctx.position_child(index, offset);
        }
        ctx.constraints()
            .constrain(flui_foundation::geometry::Size::new(100.0, 100.0))
    }

    fn paint(&self, _ctx: &mut flui_rendering::context::PaintCx<'_, Self::Arity>) {}

    fn hit_test(
        &self,
        ctx: &mut flui_rendering::context::BoxHitTestContext<'_, Self::Arity, Self::ParentData>,
    ) -> bool {
        use flui_foundation::geometry::Matrix4;
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match self.0 {
            FailedHitScope::Offset => ctx.with_offset(Offset::new(100.0, 0.0), |_| {
                panic!("failed offset hit scope")
            }),
            FailedHitScope::Transform => ctx
                .with_transform(Matrix4::scaling(2.0, 3.0, 1.0), |_| {
                    panic!("failed matrix hit scope")
                }),
            FailedHitScope::Nested => ctx.with_offset(Offset::new(100.0, 0.0), |ctx| {
                ctx.with_transform(Matrix4::scaling(2.0, 3.0, 1.0), |_| {
                    panic!("failed nested hit scope")
                });
            }),
            FailedHitScope::ChildOffset => {
                ctx.hit_test_child_at_offset(0, Offset::new(20.0, 0.0));
            }
            FailedHitScope::ChildLayoutOffset => {
                ctx.hit_test_child_at_layout_offset(0);
            }
            FailedHitScope::ChildOverride => {
                let position = *ctx.position();
                ctx.hit_test_child(0, position);
            }
        }));
        assert!(failure.is_err(), "fixture must catch its failed hit scope");
        ctx.hit_test_child_at_layout_offset(1)
    }
}

#[derive(Debug)]
struct PanickingHitLeaf;
impl flui_foundation::Diagnosticable for PanickingHitLeaf {}

impl flui_rendering::traits::RenderBox for PanickingHitLeaf {
    type Arity = flui_foundation::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_foundation::geometry::Size {
        ctx.constraints()
            .constrain(flui_foundation::geometry::Size::new(40.0, 40.0))
    }

    fn paint(&self, _ctx: &mut flui_rendering::context::PaintCx<'_, Self::Arity>) {}

    fn hit_test_transform(
        &self,
        _size: flui_foundation::geometry::Size,
    ) -> Option<flui_foundation::geometry::Matrix4> {
        Some(flui_foundation::geometry::Matrix4::scaling(2.0, 3.0, 1.0))
    }

    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<'_, Self::Arity, Self::ParentData>,
    ) -> bool {
        panic!("descendant hit test failed");
    }
}

fn assert_recovered_hit_coordinates(scope: FailedHitScope) {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let root = owner.insert(Box::new(RecoveringHitParent(scope)) as BoxedRenderObject);
    owner
        .insert_child_render_object(root, Box::new(PanickingHitLeaf))
        .expect("panicking child insert");
    let healthy = owner
        .insert_child_render_object(root, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("healthy child insert");
    let owner = laid_out(owner, root);
    for _ in 0..2 {
        let path = inspect::hit_path_with_transforms(&owner, 10.0, 10.0);
        let (_, transform) = path
            .iter()
            .find(|(id, _)| *id == healthy)
            .expect("healthy sibling remains hittable");
        let local =
            inspect::localize_hit_point(transform.expect("hit entry transform"), 10.0, 10.0)
                .expect("invertible healthy transform");
        assert_eq!(
            local,
            Offset::new(5.0, 10.0),
            "caught {scope:?} must not change the healthy child's coordinates"
        );
    }
}

pub(crate) fn caught_offset_scope_restores_hit_coordinates() {
    assert_recovered_hit_coordinates(FailedHitScope::Offset);
}
pub(crate) fn caught_matrix_scope_restores_hit_coordinates() {
    assert_recovered_hit_coordinates(FailedHitScope::Transform);
}
pub(crate) fn caught_nested_scope_restores_hit_coordinates() {
    assert_recovered_hit_coordinates(FailedHitScope::Nested);
}
pub(crate) fn caught_child_offset_scope_restores_hit_coordinates() {
    assert_recovered_hit_coordinates(FailedHitScope::ChildOffset);
}
pub(crate) fn caught_driver_node_scope_restores_hit_coordinates() {
    assert_recovered_hit_coordinates(FailedHitScope::ChildLayoutOffset);
}

pub(crate) fn caught_zero_offset_driver_node_scope_restores_hit_coordinates() {
    assert_recovered_hit_coordinates(FailedHitScope::ChildOverride);
}

pub(crate) fn nested_tiny_transforms_emit_the_correct_local_hit_point() {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let ancestor = owner.insert(Box::new(
        RenderTransform::scale(1e-4, 1e-4).with_alignment(flui_painting::Alignment::TOP_LEFT),
    ) as BoxedRenderObject);
    let child = owner
        .insert_child_render_object(
            ancestor,
            Box::new(
                RenderTransform::scale(1e-9, 1e-9)
                    .with_alignment(flui_painting::Alignment::TOP_LEFT),
            ),
        )
        .expect("nested transform inserted");
    let leaf = owner
        .insert_child_render_object(child, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("leaf inserted");
    let owner = laid_out(owner, ancestor);
    for _ in 0..2 {
        let path = inspect::hit_path_with_transforms(&owner, 5e-13, 5e-13);
        let (_, transform) = path
            .iter()
            .find(|(id, _)| *id == leaf)
            .expect("tiny transformed leaf is hittable");
        let local =
            inspect::localize_hit_point(transform.expect("recorded transform"), 5e-13, 5e-13)
                .expect("computed inverse remains usable");
        assert!(
            (local.dx - 5.0).abs() < 1e-12 && (local.dy - 5.0).abs() < 1e-12,
            "nested transforms must deliver local (5,5), got {local:?}"
        );
    }
}

#[derive(Debug)]
struct RefusalHitParent(Option<flui_foundation::geometry::Matrix4>);
impl flui_foundation::Diagnosticable for RefusalHitParent {}
impl flui_rendering::traits::RenderBox for RefusalHitParent {
    type Arity = flui_foundation::Variable;
    type ParentData = flui_rendering::parent_data::BoxParentData;
    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_foundation::geometry::Size {
        for index in 0..2 {
            ctx.layout_child(
                index,
                flui_rendering::constraints::BoxConstraints::tight(
                    flui_foundation::geometry::Size::new(40.0, 40.0),
                ),
            );
            ctx.position_child(
                index,
                if index == 0 {
                    Offset::ZERO
                } else {
                    Offset::new(5.0, 0.0)
                },
            );
        }
        ctx.constraints()
            .constrain(flui_foundation::geometry::Size::new(100.0, 100.0))
    }
    fn paint(&self, _ctx: &mut flui_rendering::context::PaintCx<'_, Self::Arity>) {}
    fn hit_test(
        &self,
        ctx: &mut flui_rendering::context::BoxHitTestContext<'_, Self::Arity, Self::ParentData>,
    ) -> bool {
        let first = match self.0 {
            Some(matrix) => {
                ctx.with_transform(matrix, |ctx| ctx.hit_test_child_at_layout_offset(0))
            }
            None => ctx.hit_test_child_at_layout_offset(0),
        };
        first || ctx.hit_test_child_at_layout_offset(1)
    }
}

#[derive(Debug)]
struct RefusedHitLeaf {
    transform: Option<flui_foundation::geometry::Matrix4>,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl flui_foundation::Diagnosticable for RefusedHitLeaf {}
impl flui_rendering::traits::RenderBox for RefusedHitLeaf {
    type Arity = flui_foundation::Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;
    fn perform_layout(
        &mut self,
        ctx: &mut flui_rendering::context::BoxLayoutContext<'_, Self::Arity, Self::ParentData>,
    ) -> flui_foundation::geometry::Size {
        ctx.constraints()
            .constrain(flui_foundation::geometry::Size::new(40.0, 40.0))
    }
    fn paint(&self, _ctx: &mut flui_rendering::context::PaintCx<'_, Self::Arity>) {}
    fn hit_test_transform(
        &self,
        _size: flui_foundation::geometry::Size,
    ) -> Option<flui_foundation::geometry::Matrix4> {
        self.transform
    }
    fn hit_test(
        &self,
        _ctx: &mut flui_rendering::context::BoxHitTestContext<'_, Self::Arity, Self::ParentData>,
    ) -> bool {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        true
    }
}

fn assert_refused_transform_preserves_sibling(
    matrix: flui_foundation::geometry::Matrix4,
    context: bool,
) {
    let mut owner = PipelineOwner::new(flui_rendering::TextContextHandle::standalone());
    let root =
        owner.insert(Box::new(RefusalHitParent(context.then_some(matrix))) as BoxedRenderObject);
    let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let invalid = owner
        .insert_child_render_object(
            root,
            Box::new(RefusedHitLeaf {
                transform: (!context).then_some(matrix),
                calls: std::sync::Arc::clone(&calls),
            }),
        )
        .expect("refused candidate inserted");
    let healthy = owner
        .insert_child_render_object(root, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("healthy sibling inserted");
    let owner = laid_out(owner, root);
    for _ in 0..2 {
        let path = inspect::hit_path_with_transforms(&owner, 10.0, 10.0);
        assert_eq!(
            calls.load(std::sync::atomic::Ordering::Relaxed),
            0,
            "inverse refusal must skip the descendant callback"
        );
        assert!(
            !path.iter().any(|(id, _)| *id == invalid),
            "refused subtree cannot publish a hit"
        );
        let (_, transform) = path
            .iter()
            .find(|(id, _)| *id == healthy)
            .expect("refusal cannot block the healthy sibling");
        assert_eq!(
            inspect::localize_hit_point(transform.expect("healthy transform"), 10.0, 10.0),
            Some(Offset::new(5.0, 10.0)),
            "sibling remains in its own coordinate space"
        );
    }
}

pub(crate) fn singular_node_transform_refuses_before_hit_and_preserves_sibling() {
    assert_refused_transform_preserves_sibling(
        flui_foundation::geometry::Matrix4::scaling(0.0, 1.0, 1.0),
        false,
    );
}
pub(crate) fn nonfinite_node_transform_refuses_before_hit_and_preserves_sibling() {
    assert_refused_transform_preserves_sibling(
        flui_foundation::geometry::Matrix4::scaling(f64::NAN, 1.0, 1.0),
        false,
    );
}
pub(crate) fn singular_context_transform_refuses_before_hit_and_preserves_sibling() {
    assert_refused_transform_preserves_sibling(
        flui_foundation::geometry::Matrix4::scaling(0.0, 1.0, 1.0),
        true,
    );
}
pub(crate) fn nonfinite_context_transform_refuses_before_hit_and_preserves_sibling() {
    assert_refused_transform_preserves_sibling(
        flui_foundation::geometry::Matrix4::scaling(f64::INFINITY, 1.0, 1.0),
        true,
    );
}
