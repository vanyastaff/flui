//! `RenderOffstage` — single-child proxy that can hide its subtree
//! completely (zero-size layout, no paint, no hit-test).
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderOffstage`](https://api.flutter.dev/flutter/rendering/RenderOffstage-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`).
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

/// A render object that, when `offstage` is true, collapses to zero
/// size, skips painting entirely, and is unreachable by hit testing.
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
            // Lay out the child at zero size so its layout state stays
            // valid (Flutter parity — the child is still part of the
            // tree, just collapsed). We then report Size::ZERO to the
            // parent.
            if ctx.child_count() > 0 {
                self.has_child = true;
                let _ = ctx.layout_child(0, BoxConstraints::tight(Size::ZERO));
                ctx.position_child(0, Offset::ZERO);
            } else {
                self.has_child = false;
            }
            Size::ZERO
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
            Size::ZERO
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
            // Recording no child marker hides the subtree.
            return;
        }
        ctx.paint_child();
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if self.offstage {
            // Unreachable while hidden — Flutter parity.
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
