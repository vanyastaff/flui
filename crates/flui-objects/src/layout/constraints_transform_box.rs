//! [`RenderConstraintsTransformBox`] — narrows the incoming constraints to a
//! single retained axis (or drops both), lays the child out under the
//! narrowed constraints, then adopts the child's size (clamped back into the
//! original constraints, so the child may overflow this box).
//!
//! # Constraint transform
//!
//! There is no general transform-function surface; the sole consumer of this
//! render object is the `UnconstrainedBox` widget (`flui-widgets`), which only
//! ever needs to free both axes or keep exactly one. The transform is
//! therefore a `constrained_axis: Option<Axis>` field — an
//! illegal-states-free shape — computed by
//! [`RenderConstraintsTransformBox::transform_constraints`]:
//!
//! | `constrained_axis` | Effect |
//! |---|---|
//! | `None` | both axes freed (`BoxConstraints::UNCONSTRAINED`) |
//! | `Some(Axis::Horizontal)` | width kept, height freed |
//! | `Some(Axis::Vertical)` | height kept, width freed |
//!
//! Sizing, the four intrinsics, and clip-on-overflow painting all go through
//! that one transform.
//!
//! # Known gap: no debug overflow indicator
//!
//! A striped debug-paint warning (and a diagnostic error) when the child
//! overflows AND `clip_behavior == Clip::None` would be the usual affordance.
//! FLUI has no debug-paint/diagnostic-error machinery anywhere in
//! `flui-rendering`/`flui-objects` (confirmed repo-wide, not just here) — see
//! `docs/ROADMAP.md` Cross.H. [`RenderConstraintsTransformBox::has_visual_overflow`] is exposed as a
//! plain queryable flag instead, matching the precedent already established
//! by [`super::fitted_box::RenderFittedBox::has_visual_overflow`].

use flui_foundation::Single;
use flui_foundation::geometry::Axis;
use flui_foundation::geometry::{Offset, Point, Rect, Size};
use flui_painting::Alignment;
use flui_painting::paint::Clip;

use flui_rendering::{
    constraints::BoxConstraints,
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
        PaintCx,
    },
    parent_data::BoxParentData,
    traits::{RenderBox, TextBaseline},
};

use super::shifted_box::AligningShiftedBox;

/// Lays its child out under a narrowed set of constraints (dropping one or
/// both axes), then adopts the child's size — clamped back into the
/// original incoming constraints, so the child may overflow this box.
///
/// See the module doc for the `constrained_axis` surface and the documented
/// debug-overflow-indicator gap.
#[derive(Debug, Clone)]
pub struct RenderConstraintsTransformBox {
    /// The axis whose incoming constraint is retained; the other axis (or
    /// both, when `None`) is freed to `(0, infinity)` before laying out the
    /// child.
    constrained_axis: Option<Axis>,
    /// How to clip the child when it overflows this box's own size.
    clip_behavior: Clip,
    /// True when the child's laid-out size exceeded this box's own size on
    /// either axis at the last layout. Reset on every layout.
    has_visual_overflow: bool,
    /// Handles child alignment and hit-testing.
    inner: AligningShiftedBox,
}

impl RenderConstraintsTransformBox {
    /// Creates the render object.
    pub fn new(alignment: Alignment, constrained_axis: Option<Axis>, clip_behavior: Clip) -> Self {
        Self {
            constrained_axis,
            clip_behavior,
            has_visual_overflow: false,
            inner: AligningShiftedBox::new(alignment),
        }
    }

    /// Returns the current alignment.
    #[inline]
    pub fn alignment(&self) -> Alignment {
        self.inner.alignment()
    }

    /// Returns the axis whose constraint is currently retained, if any.
    #[inline]
    pub fn constrained_axis(&self) -> Option<Axis> {
        self.constrained_axis
    }

    /// Returns the current clip behavior.
    #[inline]
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// Returns whether the child's laid-out size exceeded this box's own
    /// size at the last layout. Reset on every layout.
    #[inline]
    pub fn has_visual_overflow(&self) -> bool {
        self.has_visual_overflow
    }

    /// Replaces the child alignment and reports layout when changed.
    pub fn set_alignment(&mut self, alignment: Alignment) -> flui_rendering::RenderUpdateImpact {
        if self.inner.set_alignment(alignment) {
            flui_rendering::RenderUpdateImpact::LAYOUT
        } else {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    /// Replaces the retained axis and reports layout when changed.
    pub fn set_constrained_axis(
        &mut self,
        constrained_axis: Option<Axis>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.constrained_axis == constrained_axis {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.constrained_axis = constrained_axis;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Replaces the clip behavior and reports paint plus semantics when changed.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> flui_rendering::RenderUpdateImpact {
        if self.clip_behavior == clip_behavior {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.clip_behavior = clip_behavior;
        flui_rendering::RenderUpdateImpact::PAINT | flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Narrows `constraints` to the retained axis, freeing the other (or
    /// both, when `constrained_axis` is `None`) to `(0, infinity)`.
    ///
    /// See the module doc's table.
    fn transform_constraints(&self, constraints: BoxConstraints) -> BoxConstraints {
        match self.constrained_axis {
            None => BoxConstraints::UNCONSTRAINED,
            Some(Axis::Horizontal) => BoxConstraints::new(
                constraints.min_width,
                constraints.max_width,
                0.0,
                f64::INFINITY,
            ),
            Some(Axis::Vertical) => BoxConstraints::new(
                0.0,
                f64::INFINITY,
                constraints.min_height,
                constraints.max_height,
            ),
        }
    }
}

impl flui_foundation::Diagnosticable for RenderConstraintsTransformBox {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        let alignment = self.inner.alignment();
        builder.add("alignment", format!("({}, {})", alignment.x, alignment.y));
        builder.add_optional(
            "constrained_axis",
            self.constrained_axis.map(|axis| format!("{axis:?}")),
        );
        builder.add_enum("clip_behavior", self.clip_behavior);
    }
}

impl RenderBox for RenderConstraintsTransformBox {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();

        if ctx.child_count() == 0 {
            self.inner.clear_child_baselines();
            self.has_visual_overflow = false;
            return constraints.smallest();
        }

        let child_constraints = self.transform_constraints(constraints);
        let child_size = ctx.layout_child(0, child_constraints);
        let our_size = constraints.constrain(child_size);

        self.inner.align_child(ctx, our_size, child_size);
        // Overflow is the ALIGNED child rect leaving the box rect —
        // not a size-only comparison: an out-of-range alignment can push a
        // smaller-than-box child outside the box.
        let child_offset = self.inner.child_offset();
        self.has_visual_overflow = child_offset.dx < 0.0
            || child_offset.dy < 0.0
            || child_offset.dx + child_size.width > our_size.width
            || child_offset.dy + child_size.height > our_size.height;
        self.inner.record_child_baselines(ctx);
        our_size
    }

    /// Serves the live baseline recorded at the last layout, shifted by the
    /// aligned child offset (`child_baseline + offset.dy`). Without this override the trait
    /// default returns `None` and the `record_child_baselines` call above
    /// would be a dead write.
    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f64> {
        self.inner.actual_baseline(baseline)
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        self.inner.hit_test(ctx)
    }

    /// Paints the child, clipping to this box's own bounds when the child
    /// overflowed at the last layout and `clip_behavior` is not `Clip::None`.
    /// There is no debug overflow indicator otherwise (see module doc).
    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        if self.has_visual_overflow && self.clip_behavior != Clip::None {
            let bounds = Rect::from_origin_size(Point::ZERO, ctx.size());
            let clip_behavior = self.clip_behavior;
            ctx.with_clip_rect(bounds, clip_behavior, PaintCx::paint_children_in_order);
        } else {
            ctx.paint_children_in_order();
        }
    }

    // ---- intrinsic dimensions -----------------------------------------------
    //
    // A one-axis constraint is probed through `transform_constraints`, then
    // the child is queried with whatever that axis transformed to — NOT a
    // bare passthrough of the caller's `width`/`height` (unlike the sibling
    // `RenderConstrainedOverflowBox`,
    // which intentionally does pass through unmodified — see that type's own
    // intrinsics doc). A freed axis must probe the child at that axis'
    // infinite extent, exactly as layout would.

    fn compute_min_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let probe = BoxConstraints::new(0.0, f64::INFINITY, 0.0, height);
        let transformed = self.transform_constraints(probe);
        ctx.child_min_intrinsic_width(0, transformed.max_height)
    }

    fn compute_max_intrinsic_width(&self, height: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let probe = BoxConstraints::new(0.0, f64::INFINITY, 0.0, height);
        let transformed = self.transform_constraints(probe);
        ctx.child_max_intrinsic_width(0, transformed.max_height)
    }

    fn compute_min_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let probe = BoxConstraints::new(0.0, width, 0.0, f64::INFINITY);
        let transformed = self.transform_constraints(probe);
        ctx.child_min_intrinsic_height(0, transformed.max_width)
    }

    fn compute_max_intrinsic_height(&self, width: f64, ctx: &mut BoxIntrinsicsCtx<'_>) -> f64 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let probe = BoxConstraints::new(0.0, width, 0.0, f64::INFINITY);
        let transformed = self.transform_constraints(probe);
        ctx.child_max_intrinsic_height(0, transformed.max_width)
    }

    /// Dry layout uses the SAME transformed constraints as `perform_layout`,
    /// so the dry size always equals the laid-out size (the dry==committed
    /// invariant), unlike `RenderConstrainedOverflowBox`'s dry layout.
    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        if ctx.child_count() == 0 {
            return constraints.smallest();
        }
        let child_constraints = self.transform_constraints(constraints);
        let child_size = ctx.child_dry_layout(0, child_constraints);
        constraints.constrain(child_size)
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f64> {
        if ctx.child_count() == 0 {
            return None;
        }
        let child_constraints = self.transform_constraints(constraints);
        let child_size = ctx.child_dry_layout(0, child_constraints);
        let our_size = constraints.constrain(child_size);
        let child_baseline = ctx.child_dry_baseline(0, child_constraints, baseline)?;
        let child_offset: Offset = self.inner.dry_child_offset(our_size, child_size);
        Some(child_baseline + child_offset.dy)
    }
}

// ===========================================================================
// Tests
// ===========================================================================
