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

use flui_foundation::geometry::Size;
use flui_foundation::{Leaf, Variable};
use flui_objects::RenderConstrainedBox;
use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext},
    pipeline::PipelineOwner,
    storage::IntrinsicDimension,
    traits::RenderBox,
};

use crate::common::BoxedRenderObject;

// ============================================================================
// Counting test objects
// ============================================================================

/// Leaf reporting a fixed 40×40 intrinsic footprint; every compute_*
/// call bumps a shared counter so cache hits are observable.
#[derive(Debug)]
struct CountingLeaf {
    intrinsic_runs: Arc<AtomicUsize>,
    dry_runs: Arc<AtomicUsize>,
}

impl CountingLeaf {
    fn new(intrinsic_runs: Arc<AtomicUsize>, dry_runs: Arc<AtomicUsize>) -> Self {
        Self {
            intrinsic_runs,
            dry_runs,
        }
    }
}

impl flui_foundation::Diagnosticable for CountingLeaf {}

impl RenderBox for CountingLeaf {
    type Arity = Leaf;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Leaf, Self::ParentData>) -> Size {
        ctx.constraints().constrain(Size::new(40.0, 40.0))
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Leaf, Self::ParentData>) -> bool {
        false
    }

    fn compute_min_intrinsic_width(&self, _height: f64, _ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        self.intrinsic_runs.fetch_add(1, Ordering::Relaxed);
        40.0
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        self.dry_runs.fetch_add(1, Ordering::Relaxed);
        constraints.constrain(Size::new(40.0, 40.0))
    }
}

/// Variable-arity container that folds children's min intrinsic widths
/// (max-of-children, the canonical container shape) and counts its
/// perform_layout calls so dirty-walk escalation is observable.
#[derive(Debug)]
struct CountingRoot {
    layout_runs: Arc<AtomicUsize>,
}

impl CountingRoot {
    fn new(layout_runs: Arc<AtomicUsize>) -> Self {
        Self { layout_runs }
    }
}

impl flui_foundation::Diagnosticable for CountingRoot {}

impl RenderBox for CountingRoot {
    type Arity = Variable;
    type ParentData = flui_rendering::parent_data::BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Variable, Self::ParentData>,
    ) -> Size {
        self.layout_runs.fetch_add(1, Ordering::Relaxed);
        let constraints = *ctx.constraints();
        for i in 0..ctx.child_count() {
            ctx.layout_child(i, constraints.loosen());
        }
        constraints.biggest()
    }

    fn hit_test(&self, _ctx: &mut BoxHitTestContext<'_, Variable, Self::ParentData>) -> bool {
        false
    }

    fn compute_min_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        let mut max = 0.0_f64;
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
    leaf: flui_foundation::RenderId,
    intrinsic_runs: Arc<AtomicUsize>,
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
                0.0, 500.0, 0.0, 500.0,
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
    owner.set_root_constraints(Some(BoxConstraints::tight(Size::new(300.0, 300.0))));

    Fixture {
        owner,
        root,
        leaf,
        intrinsic_runs,
    }
}

// ============================================================================
// 1. Memoization per level + extent keying
// ============================================================================

pub(crate) fn intrinsic_walk_memoizes_every_level() {
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

// ============================================================================
// 3. Control pair: boundary stops the walk ⇔ cached intrinsics escalate
// ============================================================================

// ============================================================================
// 4. Dry layout: child-aware through a real object + memoized
// ============================================================================

// ============================================================================
// 5. Dry baseline: child-aware through real objects + memoized
// ============================================================================

// ============================================================================
// 6. Passthrough proxy: intrinsics + dry layout forward unchanged
// ============================================================================
