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

use flui_rendering::{
    constraints::{BoxConstraints, GrowthDirection, SliverConstraints, SliverGeometry},
    context::{BoxHitTestContext, BoxLayoutContext, SliverHitTestContext, SliverLayoutContext},
    hit_testing::HitTestResult,
    objects::{
        RenderColoredBox, RenderFlex, RenderPadding, RenderSliverIgnorePointer,
        RenderSliverOpacity, RenderSliverPadding, RenderTransform,
    },
    parent_data::{BoxParentData, SliverParentData},
    pipeline::PipelineOwner,
    protocol::{BoxProtocol, SliverProtocol},
    testing::inspect,
    traits::{RenderBox, RenderObject, RenderSliver},
    view::ScrollDirection,
};
use flui_tree::{Leaf, Variable};
use flui_types::{Matrix4, Offset, Size, geometry::px, layout::AxisDirection};

type BoxedRenderObject = Box<dyn RenderObject<BoxProtocol>>;
type BoxedSliverObject = Box<dyn RenderObject<SliverProtocol>>;

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
    inspect::hit_path(owner, x, y)
}

fn render_offset(
    owner: &flui_rendering::pipeline::PipelineOwner<flui_rendering::pipeline::phase::Layout>,
    id: flui_foundation::RenderId,
) -> Offset {
    inspect::render_offset(owner, id).expect("node exists")
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
struct SimpleRow;

impl flui_foundation::Diagnosticable for SimpleRow {}

impl RenderBox for SimpleRow {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        for i in 0..ctx.child_count() {
            let _ = ctx.layout_child(i, constraints);
            #[allow(clippy::cast_precision_loss)] // test fixture, i < 3
            ctx.position_child(i, Offset::new(px(i as f32 * 40.0), px(0.0)));
        }
        constraints.constrain(Size::new(px(120.0), px(40.0)))
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
    let row_id = owner.insert(Box::new(SimpleRow) as BoxedRenderObject);
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

#[test]
fn hit_entry_records_child_paint_offset_transform() {
    let mut owner = PipelineOwner::new();
    let padding_id = owner.insert(Box::new(RenderPadding::all(5.0)) as BoxedRenderObject);
    let child_id = owner
        .insert_child_render_object(padding_id, Box::new(RenderColoredBox::red(40.0, 40.0)))
        .expect("child insert");
    let owner = laid_out(owner, padding_id);

    let mut result = HitTestResult::new();
    owner.hit_test(Offset::new(px(20.0), px(20.0)), &mut result);

    let child_entry = result
        .path()
        .iter()
        .find(|entry| entry.target == child_id)
        .expect("child must be in hit path");

    let transform = child_entry
        .transform
        .expect("hit entry must capture the result transform stack");
    assert_ne!(
        transform,
        Matrix4::identity(),
        "laid-out child offset must contribute a non-identity transform",
    );

    let inverse = transform
        .try_inverse()
        .expect("paint-offset transform must be invertible");
    let (local_x, local_y) = inverse.transform_point(px(20.0), px(20.0));
    assert!(
        (local_x.get() - 15.0).abs() < 0.01 && (local_y.get() - 15.0).abs() < 0.01,
        "inverse transform must map global (20,20) to child-local (15,15) through 5px padding",
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

// ============================================================================
// 5. Sliver subtree hit-testing through a Box host
// ============================================================================

fn sliver_hit_constraints() -> SliverConstraints {
    SliverConstraints {
        axis_direction: AxisDirection::TopToBottom,
        cross_axis_direction: AxisDirection::LeftToRight,
        growth_direction: GrowthDirection::Forward,
        user_scroll_direction: ScrollDirection::Idle,
        scroll_offset: 0.0,
        preceding_scroll_extent: 0.0,
        overlap: 0.0,
        remaining_paint_extent: 200.0,
        cross_axis_extent: 100.0,
        viewport_main_axis_extent: 200.0,
        remaining_cache_extent: 200.0,
        cache_origin: 0.0,
    }
}

fn reverse_growth_sliver_hit_constraints() -> SliverConstraints {
    SliverConstraints {
        growth_direction: GrowthDirection::Reverse,
        ..sliver_hit_constraints()
    }
}

#[derive(Debug)]
struct SliverHitHost {
    constraints: SliverConstraints,
}

impl flui_foundation::Diagnosticable for SliverHitHost {}

impl RenderBox for SliverHitHost {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) -> Size {
        if ctx.child_count() > 0 {
            let _ = ctx.layout_sliver_child(0, self.constraints);
        }
        ctx.constraints().biggest()
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>) -> bool {
        ctx.hit_test_child(0, ctx.offset())
    }
}

#[derive(Debug)]
struct PositionedSliverHitHost {
    constraints: SliverConstraints,
    offset: Offset,
    position_child: bool,
}

impl flui_foundation::Diagnosticable for PositionedSliverHitHost {}

impl RenderBox for PositionedSliverHitHost {
    type Arity = Variable;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, BoxParentData>) -> Size {
        if ctx.child_count() > 0 {
            let _ = ctx.layout_sliver_child(0, self.constraints);
            if self.position_child {
                ctx.position_child(0, self.offset);
            }
        }
        ctx.constraints().biggest()
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Variable, BoxParentData>) -> bool {
        ctx.hit_test_child_at_layout_offset(0)
    }
}

#[derive(Debug)]
struct ConditionalOffsetSliverParent {
    position_child: bool,
    offset: Offset,
}

impl ConditionalOffsetSliverParent {
    fn new(offset: Offset) -> Self {
        Self {
            position_child: true,
            offset,
        }
    }
}

impl flui_foundation::Diagnosticable for ConditionalOffsetSliverParent {}

impl RenderSliver for ConditionalOffsetSliverParent {
    type Arity = flui_tree::Single;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, flui_tree::Single, Self::ParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();
        let child_geometry = ctx.layout_child(0, constraints);
        if self.position_child {
            ctx.position_child(0, self.offset);
        }
        SliverGeometry {
            hit_test_extent: 120.0,
            paint_extent: 120.0,
            layout_extent: 120.0,
            max_paint_extent: 120.0,
            visible: true,
            ..child_geometry
        }
    }

    fn hit_test(
        &self,
        ctx: &mut SliverHitTestContext<'_, flui_tree::Single, Self::ParentData>,
    ) -> bool {
        ctx.hit_test_child_at_layout_offset(0)
    }
}

#[derive(Debug, Default)]
struct HitLeafSliver {
    /// Cross-axis extent captured at layout, read by the `&self`-only
    /// `hit_test` (the sliver hit-test context does not carry it).
    cross_axis_extent: f32,
}

impl flui_foundation::Diagnosticable for HitLeafSliver {}

impl RenderSliver for HitLeafSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        self.cross_axis_extent = ctx.constraints().cross_axis_extent;
        SliverGeometry {
            scroll_extent: 80.0,
            paint_extent: 80.0,
            layout_extent: 80.0,
            max_paint_extent: 80.0,
            hit_test_extent: 80.0,
            visible: true,
            ..SliverGeometry::ZERO
        }
    }

    fn hit_test(&self, ctx: &mut SliverHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        // The geometry's hit_test_extent is the fixed 80.0 this double reports.
        ctx.is_within_main_axis_range(0.0, 80.0)
            && ctx.is_within_cross_axis_range(0.0, self.cross_axis_extent)
    }
}

#[derive(Debug)]
struct MainAxisBandSliver {
    hit_start: f32,
    hit_end: f32,
    /// Cross-axis extent captured at layout, read by the `&self`-only
    /// `hit_test_self` (the sliver hit-test context does not carry it).
    cross_axis_extent: f32,
}

impl MainAxisBandSliver {
    fn new(hit_start: f32, hit_end: f32) -> Self {
        Self {
            hit_start,
            hit_end,
            cross_axis_extent: 0.0,
        }
    }
}

impl flui_foundation::Diagnosticable for MainAxisBandSliver {}

impl RenderSliver for MainAxisBandSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        self.cross_axis_extent = ctx.constraints().cross_axis_extent;
        SliverGeometry {
            scroll_extent: 80.0,
            paint_extent: 80.0,
            layout_extent: 80.0,
            max_paint_extent: 80.0,
            hit_test_extent: 80.0,
            visible: true,
            ..SliverGeometry::ZERO
        }
    }

    fn hit_test_self(&self, main: f32, cross: f32) -> bool {
        main >= self.hit_start
            && main < self.hit_end
            && cross >= 0.0
            && cross < self.cross_axis_extent
    }
}

#[derive(Debug, Default)]
struct DefaultSelfHitSliver;

impl flui_foundation::Diagnosticable for DefaultSelfHitSliver {}

impl RenderSliver for DefaultSelfHitSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        SliverGeometry {
            scroll_extent: 80.0,
            paint_extent: 80.0,
            layout_extent: 80.0,
            max_paint_extent: 80.0,
            hit_test_extent: 80.0,
            visible: true,
            ..SliverGeometry::ZERO
        }
    }

    fn hit_test_self(&self, main: f32, cross: f32) -> bool {
        // The geometry's hit_test_extent is the fixed 80.0 this double reports.
        (0.0..80.0).contains(&main) && cross >= 0.0
    }
}

#[derive(Debug)]
struct OvereagerHitLeafSliver {
    /// The deliberately-overeager geometry this double reports (its
    /// `hit_test_extent` exceeds `paint_extent`).
    geometry: SliverGeometry,
}

impl OvereagerHitLeafSliver {
    fn new(paint_extent: f32, hit_test_extent: f32) -> Self {
        Self {
            geometry: SliverGeometry {
                scroll_extent: hit_test_extent.max(paint_extent),
                paint_extent,
                layout_extent: paint_extent,
                max_paint_extent: paint_extent,
                hit_test_extent,
                visible: paint_extent > 0.0,
                ..SliverGeometry::ZERO
            },
        }
    }
}

impl flui_foundation::Diagnosticable for OvereagerHitLeafSliver {}

impl RenderSliver for OvereagerHitLeafSliver {
    type Arity = Leaf;
    type ParentData = SliverParentData;

    fn perform_layout(
        &mut self,
        _ctx: &mut SliverLayoutContext<'_, Leaf, Self::ParentData>,
    ) -> SliverGeometry {
        self.geometry
    }

    fn hit_test(&self, _ctx: &mut SliverHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        true
    }
}

#[test]
fn box_host_hit_tests_sliver_proxy_subtree_leaf_first() {
    let mut owner = PipelineOwner::new();
    let host_id = owner.insert(Box::new(SliverHitHost {
        constraints: sliver_hit_constraints(),
    }) as BoxedRenderObject);
    let proxy_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            host_id,
            Box::new(RenderSliverIgnorePointer::new(false)) as BoxedSliverObject,
        )
        .expect("sliver proxy child");
    let leaf_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            proxy_id,
            Box::new(HitLeafSliver::default()) as BoxedSliverObject,
        )
        .expect("sliver leaf child");

    let owner = laid_out(owner, host_id);

    assert_eq!(
        hits(&owner, 10.0, 10.0),
        vec![leaf_id, proxy_id, host_id],
        "hit path must cross Box -> SliverIgnorePointer -> leaf Sliver and remain leaf-first",
    );
    assert!(
        hits(&owner, 10.0, 120.0).is_empty(),
        "main-axis position beyond the leaf sliver's hit extent must miss",
    );
}

#[test]
fn sliver_hit_walk_gates_each_level_before_dispatch() {
    let mut owner = PipelineOwner::new();
    let host_id = owner.insert(Box::new(SliverHitHost {
        constraints: sliver_hit_constraints(),
    }) as BoxedRenderObject);
    let leaf_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            host_id,
            Box::new(OvereagerHitLeafSliver::new(40.0, 40.0)) as BoxedSliverObject,
        )
        .expect("sliver leaf child");

    let owner = laid_out(owner, host_id);

    assert_eq!(hits(&owner, 10.0, 10.0), vec![leaf_id, host_id]);
    assert!(
        hits(&owner, 10.0, 45.0).is_empty(),
        "main-axis position outside geometry.hit_test_extent must miss \
         before the object's hit_test override can claim it",
    );
    assert!(
        hits(&owner, 120.0, 10.0).is_empty(),
        "cross-axis position outside constraints.cross_axis_extent must miss",
    );
}

#[test]
fn sliver_default_hit_test_uses_hit_test_self() {
    let mut owner = PipelineOwner::new();
    let host_id = owner.insert(Box::new(SliverHitHost {
        constraints: sliver_hit_constraints(),
    }) as BoxedRenderObject);
    let leaf_id = owner
        .render_tree_mut()
        .insert_sliver_child(host_id, Box::new(DefaultSelfHitSliver) as BoxedSliverObject)
        .expect("sliver leaf child");

    let owner = laid_out(owner, host_id);

    assert_eq!(
        hits(&owner, 10.0, 10.0),
        vec![leaf_id, host_id],
        "a sliver can override hit_test_self and rely on the default \
         RenderSliver::hit_test dispatcher",
    );
    assert!(
        hits(&owner, 10.0, 90.0).is_empty(),
        "the pipeline-level sliver hit gate still clips by geometry.hit_test_extent",
    );
}

#[test]
fn box_parent_positions_sliver_child_for_hit_testing() {
    let mut owner = PipelineOwner::new();
    let host_id = owner.insert(Box::new(PositionedSliverHitHost {
        constraints: sliver_hit_constraints(),
        offset: Offset::new(px(0.0), px(20.0)),
        position_child: true,
    }) as BoxedRenderObject);
    let leaf_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            host_id,
            Box::new(HitLeafSliver::default()) as BoxedSliverObject,
        )
        .expect("sliver leaf child");

    let owner = laid_out(owner, host_id);

    assert_eq!(
        hits(&owner, 10.0, 90.0),
        vec![leaf_id, host_id],
        "Box parent position_child must commit offsets for Sliver children \
         too: visual y=90 becomes sliver-local main=70",
    );
}

#[test]
fn box_parent_preserves_unpositioned_sliver_child_offset_across_relayout() {
    let mut owner = PipelineOwner::new();
    let host_id = owner.insert(Box::new(PositionedSliverHitHost {
        constraints: sliver_hit_constraints(),
        offset: Offset::new(px(0.0), px(20.0)),
        position_child: true,
    }) as BoxedRenderObject);
    let leaf_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            host_id,
            Box::new(HitLeafSliver::default()) as BoxedSliverObject,
        )
        .expect("sliver leaf child");

    let owner = laid_out(owner, host_id);
    assert_eq!(
        render_offset(&owner, leaf_id),
        Offset::new(px(0.0), px(20.0))
    );

    let mut owner = owner.into_idle();
    {
        let node = owner
            .render_tree_mut()
            .get_mut(host_id)
            .expect("host in tree");
        let entry = node.as_box_mut().expect("box entry");
        let host = entry
            .render_object_mut()
            .as_any_mut()
            .downcast_mut::<PositionedSliverHitHost>()
            .expect("positioned host downcast");
        host.position_child = false;
    }
    owner.mark_needs_layout(host_id);
    let mut owner = owner.into_layout();
    owner.run_layout().expect("relayout succeeds");

    assert_eq!(
        render_offset(&owner, leaf_id),
        Offset::new(px(0.0), px(20.0)),
        "a Box parent that lays out a Sliver child without re-positioning \
         it must preserve the child's previous offset",
    );
}

#[test]
fn unpositioned_sliver_child_keeps_prior_offset_across_relayout() {
    let mut owner = PipelineOwner::new();
    let host_id = owner.insert(Box::new(SliverHitHost {
        constraints: sliver_hit_constraints(),
    }) as BoxedRenderObject);
    let proxy_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            host_id,
            Box::new(ConditionalOffsetSliverParent::new(Offset::new(
                px(0.0),
                px(20.0),
            ))) as BoxedSliverObject,
        )
        .expect("sliver proxy child");
    let leaf_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            proxy_id,
            Box::new(HitLeafSliver::default()) as BoxedSliverObject,
        )
        .expect("sliver leaf child");

    let owner = laid_out(owner, host_id);
    assert_eq!(
        render_offset(&owner, leaf_id),
        Offset::new(px(0.0), px(20.0))
    );

    let mut owner = owner.into_idle();
    {
        let node = owner
            .render_tree_mut()
            .get_mut(proxy_id)
            .expect("proxy in tree");
        let entry = node.as_sliver_mut().expect("sliver entry");
        let proxy = entry
            .render_object_mut()
            .as_any_mut()
            .downcast_mut::<ConditionalOffsetSliverParent>()
            .expect("conditional proxy downcast");
        proxy.position_child = false;
    }
    owner.mark_needs_layout(host_id);
    let mut owner = owner.into_layout();
    owner.run_layout().expect("relayout succeeds");

    assert_eq!(
        render_offset(&owner, leaf_id),
        Offset::new(px(0.0), px(20.0)),
        "a sliver parent that does not call position_child on a later \
         pass must preserve the child's previously committed offset",
    );
}

#[test]
fn reverse_growth_sliver_parent_converts_child_paint_offset_for_hit_testing() {
    let mut owner = PipelineOwner::new();
    let host_id = owner.insert(Box::new(PositionedSliverHitHost {
        constraints: reverse_growth_sliver_hit_constraints(),
        offset: Offset::ZERO,
        position_child: true,
    }) as BoxedRenderObject);
    let parent_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            host_id,
            Box::new(ConditionalOffsetSliverParent::new(Offset::new(
                px(0.0),
                px(10.0),
            ))) as BoxedSliverObject,
        )
        .expect("reverse-growth sliver parent");
    let leaf_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            parent_id,
            Box::new(MainAxisBandSliver::new(0.0, 15.0)) as BoxedSliverObject,
        )
        .expect("band sliver leaf");

    let owner = laid_out(owner, host_id);
    assert_eq!(
        render_offset(&owner, leaf_id),
        Offset::new(px(0.0), px(10.0)),
        "fixture sanity: parent positioned the child 10px from the physical top",
    );
    assert_eq!(
        hits(&owner, 10.0, 80.0),
        vec![leaf_id, parent_id, host_id],
        "reverse-growth parent main=40 maps through physical y=80 and child \
         offset=10 to child main=10, inside the leading hit band",
    );
    assert!(
        hits(&owner, 10.0, 60.0).is_empty(),
        "box y=60 maps to child main=30 and must miss the leading hit band",
    );
}

#[test]
fn box_host_hit_tests_transparent_sliver_opacity_child() {
    let mut owner = PipelineOwner::new();
    let host_id = owner.insert(Box::new(SliverHitHost {
        constraints: sliver_hit_constraints(),
    }) as BoxedRenderObject);
    let opacity_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            host_id,
            Box::new(RenderSliverOpacity::transparent()) as BoxedSliverObject,
        )
        .expect("sliver opacity child");
    let leaf_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            opacity_id,
            Box::new(HitLeafSliver::default()) as BoxedSliverObject,
        )
        .expect("sliver leaf child");

    let owner = laid_out(owner, host_id);

    assert_eq!(
        hits(&owner, 10.0, 10.0),
        vec![leaf_id, opacity_id, host_id],
        "RenderSliverOpacity must not gate hit-testing on alpha; it \
         transparently delegates to its child",
    );
}

#[test]
fn sliver_padding_hit_test_extent_uses_child_hit_extent_without_after_padding() {
    let mut owner = PipelineOwner::new();
    let host_id = owner.insert(Box::new(SliverHitHost {
        constraints: sliver_hit_constraints(),
    }) as BoxedRenderObject);
    let padding_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            host_id,
            Box::new(RenderSliverPadding::symmetric(0.0, 10.0)) as BoxedSliverObject,
        )
        .expect("sliver padding child");
    owner
        .render_tree_mut()
        .insert_sliver_child(
            padding_id,
            Box::new(OvereagerHitLeafSliver::new(40.0, 100.0)) as BoxedSliverObject,
        )
        .expect("sliver leaf child");

    let owner = laid_out(owner, host_id);

    assert!(
        hits(&owner, 10.0, 120.0).is_empty(),
        "hit position beyond Flutter's padded hit-test extent must miss \
         even if the child would otherwise claim it",
    );
}

#[test]
fn sliver_padding_skips_hit_test_when_hit_test_extent_zero() {
    let mut owner = PipelineOwner::new();
    let mut constraints = sliver_hit_constraints();
    constraints.scroll_offset = 500.0;
    let host_id = owner.insert(Box::new(SliverHitHost { constraints }) as BoxedRenderObject);
    owner
        .render_tree_mut()
        .insert_sliver_child(
            host_id,
            Box::new(RenderSliverPadding::symmetric(0.0, 10.0)) as BoxedSliverObject,
        )
        .expect("padding without child");

    let owner = laid_out(owner, host_id);

    assert!(
        hits(&owner, 10.0, 10.0).is_empty(),
        "padding with zero hit_test_extent must not forward hits to a missing child",
    );
}

#[test]
fn box_host_hit_tests_sliver_padding_child_at_paint_offset() {
    let mut owner = PipelineOwner::new();
    let host_id = owner.insert(Box::new(SliverHitHost {
        constraints: sliver_hit_constraints(),
    }) as BoxedRenderObject);
    let padding_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            host_id,
            Box::new(RenderSliverPadding::symmetric(7.0, 10.0)) as BoxedSliverObject,
        )
        .expect("sliver padding child");
    let leaf_id = owner
        .render_tree_mut()
        .insert_sliver_child(
            padding_id,
            Box::new(HitLeafSliver::default()) as BoxedSliverObject,
        )
        .expect("sliver leaf child");

    let owner = laid_out(owner, host_id);

    assert_eq!(
        hits(&owner, 10.0, 15.0),
        vec![leaf_id, padding_id, host_id],
        "the point is inside the child after subtracting the padding \
         paint offset (cross=7, main=10)",
    );
    assert!(
        hits(&owner, 10.0, 5.0).is_empty(),
        "the leading main-axis padding itself is hit-transparent",
    );
    assert!(
        hits(&owner, 3.0, 15.0).is_empty(),
        "the leading cross-axis padding itself is hit-transparent",
    );
}
