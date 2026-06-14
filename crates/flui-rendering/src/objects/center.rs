//! RenderCenter - centers a single child within available space.

use flui_tree::Single;
use flui_types::{Offset, Size};

use crate::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::{
        HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability, TextBaseline,
    },
};

/// A render object that centers its child within the available space.
///
/// The child is given loose constraints (can be any size up to parent's max),
/// then positioned in the center of the available space.
///
/// # Example
///
/// ```ignore
/// let center = RenderCenter::new();
/// let mut wrapper = BoxWrapper::new(center);
/// // Add a child, then layout with constraints
/// ```
#[derive(Debug, Clone, Default)]
pub struct RenderCenter {
    /// Width factor (0.0-1.0) to shrink available width, None for full width.
    width_factor: Option<f32>,
    /// Height factor (0.0-1.0) to shrink available height, None for full
    /// height.
    height_factor: Option<f32>,
    /// Whether we have a child (tracked for hit testing).
    has_child: bool,
    /// Child offset for hit testing.
    child_offset: Offset,
}

impl RenderCenter {
    /// Creates a new center render object.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a center with width factor (shrinks available width).
    pub fn with_width_factor(mut self, factor: f32) -> Self {
        self.width_factor = Some(factor.clamp(0.0, 1.0));
        self
    }

    /// Creates a center with height factor (shrinks available height).
    pub fn with_height_factor(mut self, factor: f32) -> Self {
        self.height_factor = Some(factor.clamp(0.0, 1.0));
        self
    }

    /// Returns the width factor.
    pub fn width_factor(&self) -> Option<f32> {
        self.width_factor
    }

    /// Returns the height factor.
    pub fn height_factor(&self) -> Option<f32> {
        self.height_factor
    }

    fn dry_size(&self, constraints: &BoxConstraints, child_size: Size) -> Size {
        let width = if let Some(factor) = self.width_factor {
            child_size.width * factor
        } else {
            constraints.max_width
        };
        let height = if let Some(factor) = self.height_factor {
            child_size.height * factor
        } else {
            constraints.max_height
        };
        constraints.constrain(Size::new(width, height))
    }
}

impl flui_foundation::Diagnosticable for RenderCenter {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_optional("width_factor", self.width_factor.map(|f| format!("{f:?}")));
        builder.add_optional(
            "height_factor",
            self.height_factor.map(|f| format!("{f:?}")),
        );
    }
}

impl RenderBox for RenderCenter {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();

        tracing::debug!(
            "RenderCenter::perform_layout: constraints={:?}, child_count={}",
            constraints,
            ctx.child_count()
        );

        if ctx.child_count() > 0 {
            self.has_child = true;

            // Give child loose constraints
            let child_size = ctx.layout_single_child_loose();

            tracing::debug!("RenderCenter: child_size={:?}", child_size);

            // Calculate our size
            let width = if let Some(factor) = self.width_factor {
                child_size.width * factor
            } else {
                constraints.max_width
            };

            let height = if let Some(factor) = self.height_factor {
                child_size.height * factor
            } else {
                constraints.max_height
            };

            let size = constraints.constrain(Size::new(width, height));

            // Center the child
            self.child_offset = Offset::new(
                (size.width - child_size.width) / 2.0,
                (size.height - child_size.height) / 2.0,
            );

            tracing::debug!(
                "RenderCenter: my_size={:?}, child_offset=({}, {})",
                size,
                self.child_offset.dx,
                self.child_offset.dy
            );

            ctx.position_child(0, self.child_offset);
            size
        } else {
            self.has_child = false;
            // No child - expand to fill
            let size = constraints.biggest();
            tracing::debug!("RenderCenter: no child, size={:?}", size);
            size
        }
    }

    fn compute_min_intrinsic_width(
        &self,
        height: f32,
        ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let w_factor = self.width_factor.unwrap_or(1.0);
        ctx.child_min_intrinsic_width(0, height) * w_factor
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f32,
        ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let w_factor = self.width_factor.unwrap_or(1.0);
        ctx.child_max_intrinsic_width(0, height) * w_factor
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f32,
        ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let h_factor = self.height_factor.unwrap_or(1.0);
        ctx.child_min_intrinsic_height(0, width) * h_factor
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f32,
        ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let h_factor = self.height_factor.unwrap_or(1.0);
        ctx.child_max_intrinsic_height(0, width) * h_factor
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut crate::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        if ctx.child_count() == 0 {
            return constraints.biggest();
        }
        let child_size = ctx.child_dry_layout(0, constraints.loosen());
        self.dry_size(&constraints, child_size)
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut crate::context::BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        if ctx.child_count() == 0 {
            return None;
        }
        let child_constraints = constraints.loosen();
        let child_baseline = ctx.child_dry_baseline(0, child_constraints, baseline)?;
        let child_size = ctx.child_dry_layout(0, child_constraints);
        let size = self.dry_size(&constraints, child_size);
        let free_h = size.height.get() - child_size.height.get();
        Some(child_baseline + free_h * 0.5)
    }

    // paint() uses default no-op - Center just positions children

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }

        if self.has_child {
            ctx.hit_test_child_at_offset(0, self.child_offset)
        } else {
            false
        }
    }
}

// Mythos Step 11: explicit (default) capability opt-outs.
impl PaintEffectsCapability for RenderCenter {}
impl SemanticsCapability for RenderCenter {}
impl HotReloadCapability for RenderCenter {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_center_with_factors() {
        let center = RenderCenter::new()
            .with_width_factor(0.5)
            .with_height_factor(0.5);

        assert_eq!(center.width_factor(), Some(0.5));
        assert_eq!(center.height_factor(), Some(0.5));
    }

    #[test]
    fn test_center_default_factors() {
        let center = RenderCenter::new();
        assert_eq!(center.width_factor(), None);
        assert_eq!(center.height_factor(), None);
    }
}
