//! `RenderOffstage` — single-child proxy that lays its subtree out **at full
//! size** while hiding it from paint, hit-test and semantics.
//!
//! # Flutter equivalence
//!
//! Port of Flutter's `RenderOffstage`
//! (`packages/flutter/lib/src/rendering/proxy_box.dart:3834-3952`, master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`). The contract, read from the source:
//!
//! ```dart
//! bool get sizedByParent => offstage;                                    // :3896
//! Size computeDryLayout(c) => offstage ? c.smallest : super…;            // :3905-3910
//! void performLayout() { if (offstage) { child?.layout(constraints); }   // :3919-3925
//!                        else { super.performLayout(); } }
//! bool hitTest(…)      => !offstage && super.hitTest(…);                 // :3927-3930
//! void paint(…)        { if (offstage) return; super.paint(…); }         // :3937-3943
//! void visitChildrenForSemantics(v) { if (offstage) return; super…; }    // :3945-3951
//! ```
//!
//! The child is laid out under the **real incoming constraints** — that is the
//! whole point of `Offstage`, and what `ModalRoute.offstage` exploits to measure
//! a route at its final geometry before it is visible. Only the `RenderOffstage`
//! box itself shrinks, to `constraints.smallest`.
//!
//! # History: this was wrong, and its comment said otherwise
//!
//! This used to lay the child out at `BoxConstraints::tight(Size::ZERO)`
//! and return `Size::ZERO`, under a comment asserting "Flutter parity". Two
//! defects followed: the child never reached its real geometry, and under a
//! **tight** parent the box violated its own constraints (`constraints.smallest`
//! is the tight size, not zero). See
//! [`ADR-0020`](../../../../docs/adr/ADR-0020-transition-modal-route-seam.md).
//! Under *loose* constraints `smallest` is zero, which is why the defect
//! hid for so long.
//!
//! # Rust-native improvements
//!
//! * The `offstage` flag is a typed `bool` boundary — no `Visibility`
//!   enum overload like some Material-side ports. Flutter's source
//!   keeps the same shape, but the bool is exposed publicly without
//!   getters; here it lives behind `offstage()` / `set_offstage(...)`
//!   so the change-flag pipeline-discipline applies uniformly.
//! * Setter returns `bool` for pipeline `mark_needs_layout` short-circuit.

use flui_tree::Single;
use flui_types::{Offset, Size};

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
    /// Creates an offstage render object. Default matches Flutter:
    /// `offstage = true`.
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

    /// Updates the offstage flag; returns true if the value changed.
    pub fn set_offstage(&mut self, offstage: bool) -> bool {
        if self.offstage == offstage {
            return false;
        }
        self.offstage = offstage;
        true
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
            // `performLayout`: `child?.layout(constraints)` — the **real**
            // constraints, so the child reaches its true geometry
            // (proxy_box.dart:3919-3925). The box itself is `sizedByParent`
            // (`:3896`), so its size is `computeDryLayout` = `constraints.smallest`
            // (`:3905-3910`) — **not** `Size::ZERO`, which would violate a tight
            // parent's constraints.
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
        height: f32,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        if self.offstage {
            0.0
        } else {
            forward_min_intrinsic_width(ctx, height)
        }
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f32,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        if self.offstage {
            0.0
        } else {
            forward_max_intrinsic_width(ctx, height)
        }
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f32,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        if self.offstage {
            0.0
        } else {
            forward_min_intrinsic_height(ctx, width)
        }
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f32,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
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
            // `computeDryLayout` (proxy_box.dart:3905-3910).
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
    ) -> Option<f32> {
        if self.offstage {
            None
        } else {
            forward_dry_baseline(constraints, baseline, ctx)
        }
    }

    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Single>) {
        if self.offstage {
            // `paint` returns without painting (proxy_box.dart:3937-3943).
            return;
        }
        ctx.paint_child();
    }

    /// `visitChildrenForSemantics` returns early when offstage
    /// (proxy_box.dart:3945-3951): this node's own config is still built, its
    /// descendants are dropped from the walk.
    fn excludes_semantics_subtree(&self) -> bool {
        self.offstage
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if self.offstage {
            // `hitTest => !offstage && super.hitTest(…)` (proxy_box.dart:3927-3930).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_hidden() {
        let node = RenderOffstage::default();
        assert!(node.offstage());
    }

    #[test]
    fn constructors_round_trip_flag() {
        assert!(RenderOffstage::hidden().offstage());
        assert!(!RenderOffstage::visible().offstage());
        assert!(RenderOffstage::new(true).offstage());
        assert!(!RenderOffstage::new(false).offstage());
    }

    #[test]
    fn set_offstage_returns_change_flag() {
        let mut node = RenderOffstage::visible();
        assert!(node.set_offstage(true));
        assert!(!node.set_offstage(true)); // no-op
        assert!(node.set_offstage(false));
    }

    #[test]
    fn debug_fill_properties_lists_state() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let node = RenderOffstage::hidden();
        let mut builder = DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n == "offstage"),
            "missing diagnostic field: offstage"
        );
    }
}
