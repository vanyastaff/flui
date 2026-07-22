//! `RenderListener` — single-child proxy that receives pointer events landing
//! within its bounds and routes them to its owner-local handler.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's `RenderPointerListener`
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`): the listener
//! advertises a data-only [`PointerTarget`] that the pipeline attaches to its
//! [`HitTestEntry`](flui_rendering::hit_testing::HitTestEntry); pointer
//! dispatch resolves the target through the owner-local interaction lane and
//! invokes the registered handler (ADR-0027 — the executable callback never
//! lives in render storage). Layout and paint pass through transparently.
//! When childless it grows to the incoming maximum constraints, matching
//! Flutter's `computeSizeForNoChild`; only `hit_test` (registering self) and
//! `pointer_target` (advertising the identity) differ from a transparent
//! proxy.

use flui_tree::Single;
use flui_types::{Offset, Size};

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    hit_testing::{HitTestBehavior, PointerTarget},
    parent_data::BoxParentData,
    traits::{RenderBox, TextBaseline},
};
use flui_rendering::{context::BoxDryBaselineCtx, context::BoxDryLayoutCtx};

/// A render object that registers itself in the hit-test path and contributes
/// a data-only [`PointerTarget`], so pointer dispatch resolves and invokes the
/// owner-local handler registered for it.
///
/// `target` is `None` when the owning widget was mounted without an active
/// interaction lane (e.g. a detached harness mount); such a listener still
/// participates in hit-testing but delivers no pointer events.
///
/// `behavior` controls when the listener registers itself (Flutter's
/// `HitTestBehavior`, default [`DeferToChild`](HitTestBehavior::DeferToChild)):
///
/// * `DeferToChild` — registers only when a descendant is hit (the common case:
///   a listener wrapping a visible child; pointers landing on empty regions of
///   the listener pass through to siblings below).
/// * `Opaque` — registers for any pointer within its own bounds and blocks
///   siblings painted below.
///
/// Layout and paint are pure pass-through.
#[derive(Clone)]
pub struct RenderListener {
    target: Option<PointerTarget>,
    behavior: HitTestBehavior,
    has_child: bool,
}

impl RenderListener {
    /// Creates a listener whose hit entries carry `target`, with the given
    /// hit-test `behavior`.
    pub fn new(target: Option<PointerTarget>, behavior: HitTestBehavior) -> Self {
        Self {
            target,
            behavior,
            has_child: false,
        }
    }

    /// The pointer target advertised on this listener's hit entries.
    #[must_use]
    pub const fn target(&self) -> Option<PointerTarget> {
        self.target
    }

    /// Replaces the pointer target identity.
    pub fn set_target(&mut self, target: Option<PointerTarget>) {
        self.target = target;
    }

    /// Replaces the hit-test behavior.
    pub fn set_behavior(&mut self, behavior: HitTestBehavior) {
        self.behavior = behavior;
    }
}

impl std::fmt::Debug for RenderListener {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderListener")
            .field("has_target", &self.target.is_some())
            .field("behavior", &self.behavior)
            .field("has_child", &self.has_child)
            .finish_non_exhaustive()
    }
}

impl flui_foundation::Diagnosticable for RenderListener {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_flag("has_target", self.target.is_some(), "has_target");
        builder.add_enum("behavior", self.behavior);
        builder.add_flag("has_child", self.has_child, "has_child");
    }
}

impl RenderBox for RenderListener {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_size = ctx.layout_child(0, constraints);
            ctx.position_child(0, Offset::ZERO);
            child_size
        } else {
            self.has_child = false;
            constraints.biggest()
        }
    }

    flui_rendering::forward_single_child_intrinsics!();

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        if ctx.child_count() == 0 {
            constraints.biggest()
        } else {
            ctx.child_dry_layout(0, constraints)
        }
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        flui_rendering::context::proxy_queries::forward_dry_baseline(constraints, baseline, ctx)
    }

    // paint: default pass-through (splices the child in order).

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        // Hit-test the child first (so descendant handlers register, leaf-first).
        let child_hit = self.has_child && ctx.hit_test_child_at_offset(0, Offset::ZERO);

        let hit_target = match self.behavior {
            HitTestBehavior::Opaque => true,
            HitTestBehavior::DeferToChild | HitTestBehavior::Translucent => child_hit,
        };

        if !hit_target && self.behavior == HitTestBehavior::Translucent {
            ctx.register_self_hit_entry();
        }

        hit_target
    }

    fn pointer_target(&self) -> Option<PointerTarget> {
        self.target
    }
}
