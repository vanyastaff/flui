//! Intrinsics / dry-layout cache: memoization, invalidation, and the
//! boundary-crossing escalation (Flutter `_LayoutCacheStorage`,
//! box.dart:2840).
//!
//! Scenarios:
//! 1. a walk memoizes EVERY level — re-querying the root or the child
//!    costs zero recomputation, and the extent is part of the key;
//! 2. `mark_needs_layout` clears the caches along the walk and the
//!    next query recomputes;
//! 3. THE control pair: with no cached intrinsics a leaf invalidation
//!    stops at a relayout boundary; with cached intrinsics it
//!    escalates past the boundary to the ancestor that consumed them;
//! 4. dry layout flows child-aware through real objects and memoizes.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext},
    objects::RenderConstrainedBox,
    pipeline::PipelineOwner,
    storage::IntrinsicDimension,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};
use flui_tree::{Leaf, Variable};
use flui_types::{Size, geometry::px};

type BoxedRenderObject =
    Box<dyn flui_rendering::traits::RenderObject<flui_rendering::protocol::BoxProtocol>>;

// ============================================================================
// Counting test objects
// ============================================================================

/// Leaf reporting a fixed 40×40 intrinsic footprint; every compute_*
/// call bumps a shared counter so cache hits are observable.
#[derive(Debug)]
struct CountingLeaf {
    size: Size,
    intrinsic_runs: Arc<AtomicUsize>,
    dry_runs: Arc<AtomicUsize>,
}

impl CountingLeaf {
    fn new(intrinsic_runs: Arc<AtomicUsize>, dry_runs: Arc<AtomicUsize>) -> Self {
        Self {
            size: Size::ZERO,
            intrinsic_runs,
            dry_runs,
        }
    }
}

impl flui_foundation::Diagnosticable for CountingLeaf {}
impl PaintEffectsCapability for CountingLeaf {}
impl SemanticsCapability for CountingLeaf {}
impl HotReloadCapability for CountingLeaf {}

impl RenderBox for CountingLeaf {
    type Arity = Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, Self::ParentData>) {
        let constraints = *ctx.constraints();
        self.size = constraints.constrain(Size::new(px(40.0), px(40.0)));
        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }
    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        false
    }

    fn compute_min_intrinsic_width(&self, _height: f32, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        self.intrinsic_runs.fetch_add(1, Ordering::Relaxed);
        40.0
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        self.dry_runs.fetch_add(1, Ordering::Relaxed);
        constraints.constrain(Size::new(px(40.0), px(40.0)))
    }
}

/// Variable-arity container that folds children's min intrinsic widths
/// (max-of-children, the canonical container shape) and counts its
/// perform_layout calls so dirty-walk escalation is observable.
#[derive(Debug)]
struct CountingRoot {
    size: Size,
    layout_runs: Arc<AtomicUsize>,
}

impl CountingRoot {
    fn new(layout_runs: Arc<AtomicUsize>) -> Self {
        Self {
            size: Size::ZERO,
            layout_runs,
        }
    }
}

impl flui_foundation::Diagnosticable for CountingRoot {}
impl PaintEffectsCapability for CountingRoot {}
impl SemanticsCapability for CountingRoot {}
impl HotReloadCapability for CountingRoot {}

impl RenderBox for CountingRoot {
    type Arity = Variable;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Variable, Self::ParentData>) {
        self.layout_runs.fetch_add(1, Ordering::Relaxed);
        let constraints = *ctx.constraints();
        for i in 0..ctx.child_count() {
            ctx.layout_child(i, constraints.loosen());
        }
        self.size = constraints.biggest();
        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }
    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        false
    }

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        let mut max = 0.0f32;
        for i in 0..ctx.child_count() {
            max = max.max(ctx.child_min_intrinsic_width(i, height));
        }
        max
    }
}

// ============================================================================
// Fixtures
// ============================================================================

struct Fixture {
    owner: PipelineOwner,
    root: flui_foundation::RenderId,
    mid: flui_foundation::RenderId,
    leaf: flui_foundation::RenderId,
    intrinsic_runs: Arc<AtomicUsize>,
    dry_runs: Arc<AtomicUsize>,
    layout_runs: Arc<AtomicUsize>,
}

/// root (CountingRoot) → mid (RenderConstrainedBox, loose) → leaf
/// (CountingLeaf). The mid is a REAL object whose intrinsics forward to
/// the child constrained by its additional bounds.
fn fixture() -> Fixture {
    let intrinsic_runs = Arc::new(AtomicUsize::new(0));
    let dry_runs = Arc::new(AtomicUsize::new(0));
    let layout_runs = Arc::new(AtomicUsize::new(0));

    let mut owner = PipelineOwner::new();
    let root =
        owner.insert(Box::new(CountingRoot::new(Arc::clone(&layout_runs))) as BoxedRenderObject);
    let mid = owner
        .insert_child_render_object(
            root,
            Box::new(RenderConstrainedBox::new(BoxConstraints::new(
                px(0.0),
                px(500.0),
                px(0.0),
                px(500.0),
            ))),
        )
        .expect("mid insert");
    let leaf = owner
        .insert_child_render_object(
            mid,
            Box::new(CountingLeaf::new(
                Arc::clone(&intrinsic_runs),
                Arc::clone(&dry_runs),
            )),
        )
        .expect("leaf insert");
    owner.set_root_id(Some(root));
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(px(300.0), px(300.0)))));

    Fixture {
        owner,
        root,
        mid,
        leaf,
        intrinsic_runs,
        dry_runs,
        layout_runs,
    }
}

fn run_frame(owner: PipelineOwner) -> PipelineOwner {
    let (owner, result) = owner.run_frame();
    result.expect("frame must not error");
    owner
}

// ============================================================================
// 1. Memoization per level + extent keying
// ============================================================================

#[test]
fn intrinsic_walk_memoizes_every_level() {
    let mut f = fixture();

    let v = f
        .owner
        .box_intrinsic_dimension(f.root, IntrinsicDimension::MinWidth, 100.0)
        .expect("intrinsic query");
    assert_eq!(v, 40.0, "root folds mid's forward of the leaf's 40");
    assert_eq!(f.intrinsic_runs.load(Ordering::Relaxed), 1);

    // Same query again: the ROOT's cache answers — zero recomputation.
    let v = f
        .owner
        .box_intrinsic_dimension(f.root, IntrinsicDimension::MinWidth, 100.0)
        .expect("intrinsic re-query");
    assert_eq!(v, 40.0);
    assert_eq!(
        f.intrinsic_runs.load(Ordering::Relaxed),
        1,
        "root cache hit"
    );

    // Probing the LEAF directly also hits — the walk memoized every
    // level on the way down, not just the queried root.
    let v = f
        .owner
        .box_intrinsic_dimension(f.leaf, IntrinsicDimension::MinWidth, 100.0)
        .expect("leaf query");
    assert_eq!(v, 40.0);
    assert_eq!(
        f.intrinsic_runs.load(Ordering::Relaxed),
        1,
        "leaf cache hit"
    );

    // A different extent is a different key.
    f.owner
        .box_intrinsic_dimension(f.root, IntrinsicDimension::MinWidth, 50.0)
        .expect("new-extent query");
    assert_eq!(
        f.intrinsic_runs.load(Ordering::Relaxed),
        2,
        "the extent is part of the cache key"
    );
}

// ============================================================================
// 2. Invalidation clears the chain
// ============================================================================

#[test]
fn mark_needs_layout_invalidates_the_cached_chain() {
    let mut f = fixture();

    f.owner
        .box_intrinsic_dimension(f.root, IntrinsicDimension::MinWidth, 100.0)
        .expect("prime");
    assert_eq!(f.intrinsic_runs.load(Ordering::Relaxed), 1);

    // The leaf changes: every ancestor whose answer folded the leaf's
    // must recompute on the next query.
    f.owner.mark_needs_layout(f.leaf);

    let v = f
        .owner
        .box_intrinsic_dimension(f.root, IntrinsicDimension::MinWidth, 100.0)
        .expect("re-query after invalidation");
    assert_eq!(v, 40.0);
    assert_eq!(
        f.intrinsic_runs.load(Ordering::Relaxed),
        2,
        "the invalidation walk must clear the root's and mid's caches \
         too — a stale fold at any ancestor would answer without ever \
         reaching the changed leaf"
    );
}

// ============================================================================
// 3. Control pair: boundary stops the walk ⇔ cached intrinsics escalate
// ============================================================================

/// Marks `mid` as a relayout boundary directly on its state flags (the
/// production bit is computed during layout; the test pins the WALK's
/// reaction to the bit, not how layout derives it).
fn set_mid_boundary(f: &mut Fixture) {
    f.owner
        .render_tree_mut()
        .get_mut(f.mid)
        .expect("mid node")
        .as_box_mut()
        .expect("box entry")
        .state()
        .flags()
        .set_relayout_boundary(true);
}

#[test]
fn boundary_stops_invalidation_when_nothing_is_cached() {
    let mut f = fixture();
    f.owner = run_frame(f.owner);
    assert_eq!(f.layout_runs.load(Ordering::Relaxed), 1);

    set_mid_boundary(&mut f);

    // No intrinsic queries happened: the boundary isolates the
    // invalidation and the ROOT must not re-lay out.
    f.owner.mark_needs_layout(f.leaf);
    f.owner = run_frame(f.owner);
    assert_eq!(
        f.layout_runs.load(Ordering::Relaxed),
        1,
        "without cached intrinsic consumers the walk stops at the \
         relayout boundary"
    );
}

#[test]
fn cached_intrinsics_escalate_past_the_boundary() {
    let mut f = fixture();
    f.owner = run_frame(f.owner);
    assert_eq!(f.layout_runs.load(Ordering::Relaxed), 1);

    set_mid_boundary(&mut f);

    // The root's layout-time fold consumed the leaf's intrinsics —
    // model that consumption by priming the cache chain.
    f.owner
        .box_intrinsic_dimension(f.root, IntrinsicDimension::MinWidth, 100.0)
        .expect("prime");

    // Now the leaf changes. The boundary alone would swallow the
    // invalidation (see the control test above), but the non-empty
    // caches mean an ancestor's answer depends on the leaf — the walk
    // must escalate to the root (Flutter box.dart:2840).
    f.owner.mark_needs_layout(f.leaf);
    f.owner = run_frame(f.owner);
    assert_eq!(
        f.layout_runs.load(Ordering::Relaxed),
        2,
        "cached intrinsic consumption must carry the invalidation past \
         the relayout boundary up to the consuming ancestor"
    );
}

// ============================================================================
// 4. Dry layout: child-aware through a real object + memoized
// ============================================================================

#[test]
fn dry_layout_flows_through_real_objects_and_memoizes() {
    let mut f = fixture();

    let constraints = BoxConstraints::new(px(0.0), px(200.0), px(0.0), px(200.0));
    let size = f
        .owner
        .box_dry_layout(f.mid, constraints)
        .expect("dry layout");
    assert_eq!(
        size,
        Size::new(px(40.0), px(40.0)),
        "ConstrainedBox forwards the leaf's 40×40 dry size through its \
         loose additional constraints"
    );
    assert_eq!(f.dry_runs.load(Ordering::Relaxed), 1);

    let size = f
        .owner
        .box_dry_layout(f.mid, constraints)
        .expect("dry layout re-query");
    assert_eq!(size, Size::new(px(40.0), px(40.0)));
    assert_eq!(f.dry_runs.load(Ordering::Relaxed), 1, "memoized");

    // Different constraints are a different key.
    f.owner
        .box_dry_layout(f.mid, BoxConstraints::tight(Size::new(px(80.0), px(80.0))))
        .expect("dry layout new key");
    assert_eq!(f.dry_runs.load(Ordering::Relaxed), 2);
}
