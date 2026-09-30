//! `RenderOffstage` — single-child proxy that lays its subtree out **at full
//! size** while hiding it from paint, hit-test and semantics.
//!
//! # Contract
//!
//! * While `offstage`, the box is sized by the parent to
//!   `constraints.smallest`, `paint` returns without painting, `hit_test`
//!   refuses, and the semantics walk drops the subtree (this node's own
//!   configuration is still built).
//! * While not `offstage`, it is a transparent single-child proxy.
//!
//! The child is laid out under the **real incoming constraints** — that is the
//! whole point of `Offstage`, and what a modal route exploits to measure
//! itself at its final geometry before it is visible. Only the
//! `RenderOffstage` box itself shrinks, to `constraints.smallest`.
//!
//! Laying the child out at `BoxConstraints::tight(Size::ZERO)` and returning
//! `Size::ZERO` would be wrong twice over: the child would never reach its
//! real geometry, and under a **tight** parent the box would violate its own
//! constraints (`constraints.smallest` is the tight size, not zero). See
//! [`ADR-0020`](../../../../docs/adr/ADR-0020-transition-modal-route-seam.md).
//! Under *loose* constraints `smallest` is zero, which is how such a defect
//! hides.
//!
//! # Rust-native improvements
//!
//! * The `offstage` flag is a typed `bool` boundary — no `Visibility`
//!   enum overload. It lives behind `offstage()` / `set_offstage(...)`
//!   so the change-flag pipeline-discipline applies uniformly.
//! * The setter returns the exact pipeline impact.

use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Size};

use flui_rendering::{
    constraints::BoxConstraints,
    context::proxy_queries::{
        forward_dry_baseline, forward_dry_layout, forward_max_intrinsic_height,
        forward_max_intrinsic_width, forward_min_intrinsic_height, forward_min_intrinsic_width,
    },
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::{RenderBox, TextBaseline},
};

/// A render object that, when `offstage` is true, lays its child out under the
/// real incoming constraints but takes `constraints.smallest` for itself, skips
/// painting entirely, is unreachable by hit testing, and drops its subtree from
/// the semantics walk.
///
/// When `offstage` is false, it behaves as a transparent single-child
/// proxy: child receives the parent's constraints, the box adopts the
/// child's size, and paint/hit-test delegate to the child.
#[derive(Debug, Clone)]
pub struct RenderOffstage {
    offstage: bool,
    has_child: bool,
}

impl RenderOffstage {
    /// Creates an offstage render object with the given `offstage` flag.
    pub const fn new(offstage: bool) -> Self {
        Self {
            offstage,
            has_child: false,
        }
    }

    /// Creates an offstage render object that is currently hidden.
    pub const fn hidden() -> Self {
        Self::new(true)
    }

    /// Creates an offstage render object that is currently visible.
    pub const fn visible() -> Self {
        Self::new(false)
    }

    /// Returns whether the subtree is currently offstage (hidden).
    #[inline]
    pub fn offstage(&self) -> bool {
        self.offstage
    }

    /// Updates the offstage flag and reports layout plus semantics when changed.
    pub fn set_offstage(&mut self, offstage: bool) -> flui_rendering::RenderUpdateImpact {
        if self.offstage == offstage {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.offstage = offstage;
        flui_rendering::RenderUpdateImpact::LAYOUT | flui_rendering::RenderUpdateImpact::SEMANTICS
    }
}

impl Default for RenderOffstage {
    fn default() -> Self {
        Self::hidden()
    }
}

impl flui_foundation::Diagnosticable for RenderOffstage {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_flag("offstage", self.offstage, "offstage");
    }
}

impl RenderBox for RenderOffstage {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        if self.offstage {
            // The child gets the **real** constraints, so it reaches its true
            // geometry. The box itself is sized by the parent to
            // `constraints.smallest` — **not** `Size::ZERO`, which would
            // violate a tight parent's constraints.
            let constraints = *ctx.constraints();
            if ctx.child_count() > 0 {
                self.has_child = true;
                let _ = ctx.layout_child(0, constraints);
                ctx.position_child(0, Offset::ZERO);
            } else {
                self.has_child = false;
            }
            constraints.smallest()
        } else {
            // Transparent proxy.
            let constraints = *ctx.constraints();
            if ctx.child_count() > 0 {
                self.has_child = true;
                let child_size = ctx.layout_child(0, constraints);
                ctx.position_child(0, Offset::ZERO);
                child_size
            } else {
                self.has_child = false;
                constraints.smallest()
            }
        }
    }

    fn compute_min_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        if self.offstage {
            0.0
        } else {
            forward_min_intrinsic_width(ctx, height)
        }
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        if self.offstage {
            0.0
        } else {
            forward_max_intrinsic_width(ctx, height)
        }
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        if self.offstage {
            0.0
        } else {
            forward_min_intrinsic_height(ctx, width)
        }
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        if self.offstage {
            0.0
        } else {
            forward_max_intrinsic_height(ctx, width)
        }
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut flui_rendering::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        if self.offstage {
            // Same size `perform_layout` commits while offstage.
            constraints.smallest()
        } else {
            forward_dry_layout(constraints, ctx)
        }
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut flui_rendering::context::BoxDryBaselineCtx<'_>,
    ) -> Option<f64> {
        if self.offstage {
            None
        } else {
            forward_dry_baseline(constraints, baseline, ctx)
        }
    }

    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Single>) {
        if self.offstage {
            // Nothing is painted while offstage.
            return;
        }
        ctx.paint_child();
    }

    /// While offstage this node's own config is still built, but its
    /// descendants are dropped from the semantics walk.
    fn excludes_semantics_subtree(&self) -> bool {
        self.offstage
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if self.offstage {
            // Unreachable by hit testing while offstage.
            return false;
        }
        if !ctx.is_within_own_size() {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================
