//! [`RenderConstraintsTransformBox`] — narrows the incoming constraints to a
//! single retained axis (or drops both), lays the child out under the
//! narrowed constraints, then adopts the child's size (clamped back into the
//! original constraints, so the child may overflow this box).
//!
//! # Flutter equivalence
//!
//! Flutter's `RenderConstraintsTransformBox` (`rendering/shifted_box.dart`,
//! tag `3.44.0`) accepts an arbitrary `BoxConstraintsTransform` — a
//! `BoxConstraints -> BoxConstraints` function — of which the
//! `ConstraintsTransformBox` widget predefines seven named variants
//! (`unmodified`, `unconstrained`, `widthUnconstrained`, `heightUnconstrained`,
//! `maxWidthUnconstrained`, `maxHeightUnconstrained`, `maxUnconstrained`).
//! FLUI has no `ConstraintsTransformBox` widget and no general
//! transform-function surface; the sole consumer of this render object is the
//! `UnconstrainedBox` widget (`flui-widgets`), which only ever needs the
//! three variants `Axis`-shaped: free both axes, or keep exactly one. The
//! transform is therefore narrowed from an arbitrary function to a
//! `constrained_axis: Option<Axis>` field — a Rust-native, illegal-states-free
//! shape (Prime Directive #1: structure is Rust-native, behavior stays
//! loyal) — computed by [`Self::transform_constraints`]:
//!
//! | `constrained_axis` | Flutter transform | Effect |
//! |---|---|---|
//! | `None` | `unconstrained` | both axes freed (`BoxConstraints.UNCONSTRAINED`) |
//! | `Some(Axis::Horizontal)` | `heightUnconstrained` (→ `constraints.widthConstraints()`) | width kept, height freed |
//! | `Some(Axis::Vertical)` | `widthUnconstrained` (→ `constraints.heightConstraints()`) | height kept, width freed |
//!
//! Sizing (`performLayout`/`computeDryLayout`), the four intrinsics
//! (`computeMin/MaxIntrinsicWidth/Height`), and clip-on-overflow painting are
//! ported behavior-faithfully from the Flutter source above.
//!
//! # Known gap: no debug overflow indicator
//!
//! Flutter's `RenderConstraintsTransformBox` mixes in `DebugOverflowIndicatorMixin`
//! to paint the yellow-and-black striped warning (and emit a `FlutterError`
//! diagnostic) when the child overflows AND `clipBehavior == Clip.none`. FLUI
//! has no equivalent debug-paint/diagnostic-error machinery anywhere in
//! `flui-rendering`/`flui-objects` (confirmed repo-wide, not just here) — see
//! `docs/ROADMAP.md` Cross.H. [`Self::has_visual_overflow`] is exposed as a
//! plain queryable flag instead, matching the precedent already established
//! by [`super::fitted_box::RenderFittedBox::has_visual_overflow`].

use flui_tree::Single;
use flui_types::{
    Alignment, Axis, Offset, Pixels, Point, Rect, Size, geometry::px, painting::Clip,
};

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
/// Flutter parity: `RenderConstraintsTransformBox` in `shifted_box.dart`
/// (see the module doc for the narrowed `constrained_axis` surface and the
/// documented debug-overflow-indicator gap).
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

    /// Replaces the child alignment; returns `true` if the value changed.
    pub fn set_alignment(&mut self, alignment: Alignment) -> bool {
        self.inner.set_alignment(alignment)
    }

    /// Replaces the retained axis; returns `true` if the value changed.
    pub fn set_constrained_axis(&mut self, constrained_axis: Option<Axis>) -> bool {
        if self.constrained_axis == constrained_axis {
            return false;
        }
        self.constrained_axis = constrained_axis;
        true
    }

    /// Replaces the clip behavior; returns `true` if the value changed.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> bool {
        if self.clip_behavior == clip_behavior {
            return false;
        }
        self.clip_behavior = clip_behavior;
        true
    }

    /// Narrows `constraints` to the retained axis, freeing the other (or
    /// both, when `constrained_axis` is `None`) to `(0, infinity)`.
    ///
    /// Mirrors Flutter's three `UnconstrainedBox`-reachable
    /// `BoxConstraintsTransform`s — see the module doc's mapping table.
    fn transform_constraints(&self, constraints: BoxConstraints) -> BoxConstraints {
        match self.constrained_axis {
            None => BoxConstraints::UNCONSTRAINED,
            Some(Axis::Horizontal) => BoxConstraints::new(
                constraints.min_width,
                constraints.max_width,
                Pixels::ZERO,
                Pixels::INFINITY,
            ),
            Some(Axis::Vertical) => BoxConstraints::new(
                Pixels::ZERO,
                Pixels::INFINITY,
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
        // Flutter's `RelativeRect.fromRect(container, child).hasInsets` —
        // not a size-only comparison: an out-of-range alignment can push a
        // smaller-than-box child outside the box.
        let child_offset = self.inner.child_offset();
        self.has_visual_overflow = child_offset.dx < px(0.0)
            || child_offset.dy < px(0.0)
            || child_offset.dx + child_size.width > our_size.width
            || child_offset.dy + child_size.height > our_size.height;
        self.inner.record_child_baselines(ctx);
        our_size
    }

    /// Serves the live baseline recorded at the last layout, shifted by the
    /// aligned child offset — Flutter's `RenderShiftedBox` contract
    /// (`child_baseline + offset.dy`). Without this override the trait
    /// default returns `None` and the `record_child_baselines` call above
    /// would be a dead write.
    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.inner.actual_baseline(baseline)
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        self.inner.hit_test(ctx)
    }

    /// Paints the child, clipping to this box's own bounds when the child
    /// overflowed at the last layout and `clip_behavior` is not `Clip::None`.
    ///
    /// Flutter parity: `RenderConstraintsTransformBox.paint` wraps the child
    /// paint in `pushClipRect` under the same `_isOverflowing &&
    /// clipBehavior != Clip.none` guard; the debug-mode overflow indicator
    /// painted in the `else` branch has no FLUI equivalent (see module doc).
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
    // Flutter parity: `RenderConstraintsTransformBox` probes a one-axis
    // constraint through `constraintsTransform`, then queries the child with
    // whatever that axis transformed to — NOT a bare passthrough of the
    // caller's `width`/`height` (unlike the sibling `RenderConstrainedOverflowBox`,
    // which intentionally does pass through unmodified — see that type's own
    // intrinsics doc). A freed axis must probe the child at that axis'
    // infinite extent, exactly as layout would.

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let probe = BoxConstraints::new(Pixels::ZERO, Pixels::INFINITY, Pixels::ZERO, px(height));
        let transformed = self.transform_constraints(probe);
        ctx.child_min_intrinsic_width(0, transformed.max_height.get())
    }

    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let probe = BoxConstraints::new(Pixels::ZERO, Pixels::INFINITY, Pixels::ZERO, px(height));
        let transformed = self.transform_constraints(probe);
        ctx.child_max_intrinsic_width(0, transformed.max_height.get())
    }

    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let probe = BoxConstraints::new(Pixels::ZERO, px(width), Pixels::ZERO, Pixels::INFINITY);
        let transformed = self.transform_constraints(probe);
        ctx.child_min_intrinsic_height(0, transformed.max_width.get())
    }

    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        let probe = BoxConstraints::new(Pixels::ZERO, px(width), Pixels::ZERO, Pixels::INFINITY);
        let transformed = self.transform_constraints(probe);
        ctx.child_max_intrinsic_height(0, transformed.max_width.get())
    }

    /// Dry layout uses the SAME transformed constraints as `perform_layout`,
    /// so FLUI's dry size always equals its laid-out size (the dry==committed
    /// invariant). Flutter's own `computeDryLayout` does the same here (unlike
    /// `RenderConstrainedOverflowBox`, which has a documented Flutter-only
    /// dry/layout inconsistency this crate deliberately does not replicate).
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
    ) -> Option<f32> {
        if ctx.child_count() == 0 {
            return None;
        }
        let child_constraints = self.transform_constraints(constraints);
        let child_size = ctx.child_dry_layout(0, child_constraints);
        let our_size = constraints.constrain(child_size);
        let child_baseline = ctx.child_dry_baseline(0, child_constraints, baseline)?;
        let child_offset: Offset = self.inner.dry_child_offset(our_size, child_size);
        Some(child_baseline + child_offset.dy.get())
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn bc(min_w: f32, max_w: f32, min_h: f32, max_h: f32) -> BoxConstraints {
        BoxConstraints::new(px(min_w), px(max_w), px(min_h), px(max_h))
    }

    #[test]
    fn transform_constraints_none_frees_both_axes() {
        let node = RenderConstraintsTransformBox::new(Alignment::CENTER, None, Clip::None);
        let transformed = node.transform_constraints(bc(10.0, 200.0, 5.0, 100.0));
        assert_eq!(transformed, BoxConstraints::UNCONSTRAINED);
    }

    #[test]
    fn transform_constraints_horizontal_keeps_width_frees_height() {
        let node = RenderConstraintsTransformBox::new(
            Alignment::CENTER,
            Some(Axis::Horizontal),
            Clip::None,
        );
        let transformed = node.transform_constraints(bc(10.0, 200.0, 5.0, 100.0));
        assert_eq!(transformed.min_width, px(10.0));
        assert_eq!(transformed.max_width, px(200.0));
        assert_eq!(transformed.min_height, Pixels::ZERO);
        assert_eq!(transformed.max_height, Pixels::INFINITY);
    }

    #[test]
    fn transform_constraints_vertical_keeps_height_frees_width() {
        let node =
            RenderConstraintsTransformBox::new(Alignment::CENTER, Some(Axis::Vertical), Clip::None);
        let transformed = node.transform_constraints(bc(10.0, 200.0, 5.0, 100.0));
        assert_eq!(transformed.min_width, Pixels::ZERO);
        assert_eq!(transformed.max_width, Pixels::INFINITY);
        assert_eq!(transformed.min_height, px(5.0));
        assert_eq!(transformed.max_height, px(100.0));
    }

    #[test]
    fn setters_return_change_flag() {
        let mut node = RenderConstraintsTransformBox::new(Alignment::CENTER, None, Clip::None);
        assert!(node.set_constrained_axis(Some(Axis::Horizontal)));
        assert!(!node.set_constrained_axis(Some(Axis::Horizontal)));
        assert!(node.set_clip_behavior(Clip::AntiAlias));
        assert!(!node.set_clip_behavior(Clip::AntiAlias));
        assert!(node.set_alignment(Alignment::TOP_LEFT));
    }

    #[test]
    fn defaults_report_no_overflow_before_layout() {
        let node = RenderConstraintsTransformBox::new(Alignment::CENTER, None, Clip::None);
        assert!(!node.has_visual_overflow());
    }
}
