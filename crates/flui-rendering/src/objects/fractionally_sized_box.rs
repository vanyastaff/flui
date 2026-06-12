//! `RenderFractionallySizedBox` — sizes the child as a fraction of the
//! parent's available space, and aligns it inside the parent.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderFractionallySizedOverflowBox`](https://api.flutter.dev/flutter/rendering/RenderFractionallySizedOverflowBox-class.html)
//! restricted to the non-overflow case (matching the `FractionallySizedBox`
//! widget contract).
//!
//! # Rust-native improvements
//!
//! * Fraction factors are typed via [`FractionFactor`], a newtype that
//!   forbids negative values at the API boundary (Flutter accepts any
//!   `double` and silently zeroes-out negatives at runtime).
//! * `width_factor`/`height_factor` are `Option<FractionFactor>` —
//!   matching Flutter's `null = inherit parent constraint` semantics
//!   without overloading `0.0` as a magic sentinel.
//! * Alignment uses [`flui_types::Alignment`] (`x`,`y` ∈ `[-1, 1]`) rather
//!   than the painting-side parallel definition, keeping the alignment
//!   math consistent with `RenderTransform` / `RenderCenter`.

use flui_tree::Single;
use flui_types::{Alignment, Offset, Pixels, Point, Rect, Size, geometry::px};

use crate::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderBox, SemanticsCapability},
};

/// A non-negative fraction used as a sizing factor.
///
/// `0.0` means "collapse this axis", `1.0` means "match the parent",
/// `2.0` would mean "twice the parent" — values above 1.0 are accepted
/// because that's how `FractionallySizedBox` is used in practice with the
/// overflow flag implicit to the parent layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FractionFactor(f32);

impl FractionFactor {
    /// 0.0 — collapse.
    pub const ZERO: Self = Self(0.0);
    /// 0.5 — half of the parent.
    pub const HALF: Self = Self(0.5);
    /// 1.0 — match the parent.
    pub const FULL: Self = Self(1.0);

    /// Creates a non-negative, finite fraction factor.
    ///
    /// Returns `None` for negative, NaN, or infinite inputs.
    #[must_use]
    pub fn new(value: f32) -> Option<Self> {
        if value.is_finite() && value >= 0.0 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Creates a fraction factor without validation (debug-asserted).
    #[must_use]
    pub const fn new_unchecked(value: f32) -> Self {
        debug_assert!(value.is_finite() && value >= 0.0, "invalid fraction factor");
        Self(value)
    }

    /// Returns the underlying f32 value.
    #[inline]
    #[must_use]
    pub const fn value(self) -> f32 {
        self.0
    }
}

impl From<FractionFactor> for f32 {
    fn from(value: FractionFactor) -> Self {
        value.0
    }
}

/// A render object that sizes its child as a fraction of the available
/// space, optionally collapsing axes the parent leaves unbounded.
///
/// # Sizing algorithm
///
/// For each axis (width and height) independently:
///
/// 1. If a `factor` is set, the child's tight constraint on that axis is
///    `parent_max × factor` (or `parent_min × factor` if max is infinite).
/// 2. If a `factor` is not set, the child uses the parent's incoming
///    constraint for that axis untouched.
///
/// Then the child's overall box size = `incoming.constrain(child_size)` and
/// the child is positioned according to the [`alignment`](Self::alignment)
/// inside that box.
///
/// # Example
///
/// ```ignore
/// use flui_rendering::objects::{FractionFactor, RenderFractionallySizedBox};
/// use flui_types::Alignment;
///
/// // Child takes 50% width × 75% height of the parent, top-centered.
/// let node = RenderFractionallySizedBox::new()
///     .with_width_factor(FractionFactor::HALF)
///     .with_height_factor(FractionFactor::new(0.75).unwrap())
///     .with_alignment(Alignment::TOP_CENTER);
/// ```
#[derive(Debug, Clone)]
pub struct RenderFractionallySizedBox {
    /// Width sizing factor (None = inherit parent's width constraint).
    width_factor: Option<FractionFactor>,
    /// Height sizing factor (None = inherit parent's height constraint).
    height_factor: Option<FractionFactor>,
    /// Alignment of the child within the parent box.
    alignment: Alignment,
    /// Final size after layout.
    size: Size,
    /// Cached child offset.
    child_offset: Offset,
    /// Whether we have a child (tracked for hit testing).
    has_child: bool,
}

impl RenderFractionallySizedBox {
    /// Creates a fractionally-sized box with no factors (child inherits
    /// parent's constraints) and center alignment.
    pub const fn new() -> Self {
        Self {
            width_factor: None,
            height_factor: None,
            alignment: Alignment::CENTER,
            size: Size::ZERO,
            child_offset: Offset::ZERO,
            has_child: false,
        }
    }

    /// Sets the width factor (builder).
    #[must_use]
    pub const fn with_width_factor(mut self, factor: FractionFactor) -> Self {
        self.width_factor = Some(factor);
        self
    }

    /// Sets the height factor (builder).
    #[must_use]
    pub const fn with_height_factor(mut self, factor: FractionFactor) -> Self {
        self.height_factor = Some(factor);
        self
    }

    /// Sets the alignment (builder).
    #[must_use]
    pub const fn with_alignment(mut self, alignment: Alignment) -> Self {
        self.alignment = alignment;
        self
    }

    /// Returns the current width factor.
    #[inline]
    pub fn width_factor(&self) -> Option<FractionFactor> {
        self.width_factor
    }

    /// Returns the current height factor.
    #[inline]
    pub fn height_factor(&self) -> Option<FractionFactor> {
        self.height_factor
    }

    /// Returns the current alignment.
    #[inline]
    pub fn alignment(&self) -> Alignment {
        self.alignment
    }

    /// Sets the width factor; returns true if the value changed.
    pub fn set_width_factor(&mut self, factor: Option<FractionFactor>) -> bool {
        if self.width_factor == factor {
            return false;
        }
        self.width_factor = factor;
        true
    }

    /// Sets the height factor; returns true if the value changed.
    pub fn set_height_factor(&mut self, factor: Option<FractionFactor>) -> bool {
        if self.height_factor == factor {
            return false;
        }
        self.height_factor = factor;
        true
    }

    /// Sets the alignment; returns true if the value changed.
    pub fn set_alignment(&mut self, alignment: Alignment) -> bool {
        if self.alignment == alignment {
            return false;
        }
        self.alignment = alignment;
        true
    }

    /// Computes the tight constraints to pass to the child for these
    /// incoming constraints.
    /// Width factor as a bare multiplier, `1.0` when unset (Flutter's
    /// `_widthFactor ?? 1.0` in the intrinsic formulas).
    fn width_factor_or_one(&self) -> f32 {
        self.width_factor.map_or(1.0, |f| f.value())
    }

    /// Height factor as a bare multiplier, `1.0` when unset.
    fn height_factor_or_one(&self) -> f32 {
        self.height_factor.map_or(1.0, |f| f.value())
    }

    fn child_constraints(&self, incoming: BoxConstraints) -> BoxConstraints {
        // Width axis.
        let (min_w, max_w) = match self.width_factor {
            Some(factor) => {
                let base = if incoming.max_width.get().is_finite() {
                    incoming.max_width
                } else {
                    incoming.min_width
                };
                let target = px(base.get() * factor.value());
                (target, target)
            }
            None => (incoming.min_width, incoming.max_width),
        };
        // Height axis.
        let (min_h, max_h) = match self.height_factor {
            Some(factor) => {
                let base = if incoming.max_height.get().is_finite() {
                    incoming.max_height
                } else {
                    incoming.min_height
                };
                let target = px(base.get() * factor.value());
                (target, target)
            }
            None => (incoming.min_height, incoming.max_height),
        };
        BoxConstraints::new(min_w, max_w, min_h, max_h)
    }

    /// Resolves the child's top-left offset inside `box_size` for a child
    /// of size `child_size`.
    fn align_child(&self, box_size: Size, child_size: Size) -> Offset {
        // Alignment maps [-1, 1] → [0, free_space]:
        //   normalized = (x + 1) / 2
        //   offset = normalized × (box - child)
        let free_w = box_size.width - child_size.width;
        let free_h = box_size.height - child_size.height;
        let dx = Pixels::new(free_w.get() * (self.alignment.x + 1.0) * 0.5);
        let dy = Pixels::new(free_h.get() * (self.alignment.y + 1.0) * 0.5);
        Offset::new(dx, dy)
    }
}

impl Default for RenderFractionallySizedBox {
    fn default() -> Self {
        Self::new()
    }
}

impl flui_foundation::Diagnosticable for RenderFractionallySizedBox {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add(
            "width_factor",
            self.width_factor
                .map(|f| format!("{}", f.value()))
                .unwrap_or_else(|| "unset".to_string()),
        );
        builder.add(
            "height_factor",
            self.height_factor
                .map(|f| format!("{}", f.value()))
                .unwrap_or_else(|| "unset".to_string()),
        );
        builder.add(
            "alignment",
            format!("({}, {})", self.alignment.x, self.alignment.y),
        );
    }
}

impl RenderBox for RenderFractionallySizedBox {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) {
        let incoming = *ctx.constraints();

        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_constraints = self.child_constraints(incoming);
            let child_size = ctx.layout_child(0, child_constraints);
            // Our box = the parent's tightest acceptable size that wraps
            // the child. With factors set, the child IS that size; without
            // factors, we use the child as-is.
            self.size = incoming.constrain(child_size);
            self.child_offset = self.align_child(self.size, child_size);
            ctx.position_child(0, self.child_offset);
        } else {
            self.has_child = false;
            // Without a child, our size is determined by the factors alone.
            let computed = self.child_constraints(incoming);
            self.size = incoming.constrain(Size::new(computed.min_width, computed.min_height));
            self.child_offset = Offset::ZERO;
        }

        ctx.complete_with_size(self.size);
    }

    fn size(&self) -> &Size {
        &self.size
    }

    fn size_mut(&mut self) -> &mut Size {
        &mut self.size
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_size(self.size.width, self.size.height) {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, self.child_offset)
        } else {
            false
        }
    }

    fn box_paint_bounds(&self) -> Rect {
        Rect::from_origin_size(Point::ZERO, self.size)
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut crate::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        let computed = self.child_constraints(constraints);
        if ctx.child_count() > 0 {
            constraints.constrain(ctx.child_dry_layout(0, computed))
        } else {
            constraints.constrain(Size::new(computed.min_width, computed.min_height))
        }
    }

    // Flutter parity: shifted_box.dart `RenderFractionallySizedOverflowBox`
    // — the child is probed at the OTHER axis's scaled extent (infinity
    // absorption keeps an unbounded extent unbounded), and the answer is
    // divided back by this axis's factor.

    fn compute_min_intrinsic_width(
        &self,
        height: f32,
        ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        let result = if ctx.child_count() > 0 {
            ctx.child_min_intrinsic_width(0, height * self.height_factor_or_one())
        } else {
            0.0
        };
        debug_assert!(
            result.is_finite(),
            "child min intrinsic width must be finite"
        );
        result / self.width_factor_or_one()
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f32,
        ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        let result = if ctx.child_count() > 0 {
            ctx.child_max_intrinsic_width(0, height * self.height_factor_or_one())
        } else {
            0.0
        };
        debug_assert!(
            result.is_finite(),
            "child max intrinsic width must be finite"
        );
        result / self.width_factor_or_one()
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f32,
        ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        let result = if ctx.child_count() > 0 {
            ctx.child_min_intrinsic_height(0, width * self.width_factor_or_one())
        } else {
            0.0
        };
        debug_assert!(
            result.is_finite(),
            "child min intrinsic height must be finite"
        );
        result / self.height_factor_or_one()
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f32,
        ctx: &mut crate::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
        let result = if ctx.child_count() > 0 {
            ctx.child_max_intrinsic_height(0, width * self.width_factor_or_one())
        } else {
            0.0
        };
        debug_assert!(
            result.is_finite(),
            "child max intrinsic height must be finite"
        );
        result / self.height_factor_or_one()
    }
}

// Mythos Step 11: explicit (default) capability opt-outs.
impl PaintEffectsCapability for RenderFractionallySizedBox {}
impl SemanticsCapability for RenderFractionallySizedBox {}
impl HotReloadCapability for RenderFractionallySizedBox {}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn bc(min_w: f32, max_w: f32, min_h: f32, max_h: f32) -> BoxConstraints {
        BoxConstraints::new(px(min_w), px(max_w), px(min_h), px(max_h))
    }

    // ---------- FractionFactor newtype ------------------------------------

    #[test]
    fn factor_rejects_negative_or_non_finite() {
        assert!(FractionFactor::new(-0.5).is_none());
        assert!(FractionFactor::new(f32::NAN).is_none());
        assert!(FractionFactor::new(f32::INFINITY).is_none());
    }

    #[test]
    fn factor_accepts_zero_and_above_one() {
        assert!(FractionFactor::new(0.0).is_some());
        assert!(FractionFactor::new(2.5).is_some());
    }

    #[test]
    fn factor_constants_are_typed() {
        assert_eq!(FractionFactor::ZERO.value(), 0.0);
        assert_eq!(FractionFactor::HALF.value(), 0.5);
        assert_eq!(FractionFactor::FULL.value(), 1.0);
    }

    // ---------- builder ergonomics ----------------------------------------

    #[test]
    fn defaults_have_no_factors_and_center_alignment() {
        let node = RenderFractionallySizedBox::default();
        assert!(node.width_factor().is_none());
        assert!(node.height_factor().is_none());
        assert_eq!(node.alignment(), Alignment::CENTER);
    }

    #[test]
    fn builder_chain_assembles_node() {
        let node = RenderFractionallySizedBox::new()
            .with_width_factor(FractionFactor::HALF)
            .with_height_factor(FractionFactor::FULL)
            .with_alignment(Alignment::TOP_LEFT);
        assert_eq!(node.width_factor(), Some(FractionFactor::HALF));
        assert_eq!(node.height_factor(), Some(FractionFactor::FULL));
        assert_eq!(node.alignment(), Alignment::TOP_LEFT);
    }

    // ---------- child_constraints ----------------------------------------

    #[test]
    fn no_factors_passes_constraints_through() {
        let node = RenderFractionallySizedBox::new();
        let cc = node.child_constraints(bc(10.0, 100.0, 5.0, 50.0));
        assert_eq!(cc.min_width, px(10.0));
        assert_eq!(cc.max_width, px(100.0));
        assert_eq!(cc.min_height, px(5.0));
        assert_eq!(cc.max_height, px(50.0));
    }

    #[test]
    fn width_factor_tightens_to_fraction_of_max() {
        let node = RenderFractionallySizedBox::new().with_width_factor(FractionFactor::HALF);
        let cc = node.child_constraints(bc(0.0, 200.0, 0.0, 100.0));
        assert_eq!(cc.min_width, px(100.0));
        assert_eq!(cc.max_width, px(100.0));
        // Height is untouched.
        assert_eq!(cc.max_height, px(100.0));
    }

    #[test]
    fn factor_falls_back_to_min_when_max_unbounded() {
        let node = RenderFractionallySizedBox::new().with_height_factor(FractionFactor::FULL);
        let cc = node.child_constraints(bc(0.0, 200.0, 30.0, f32::INFINITY));
        // height_factor=1.0 with infinite max → tight at min_height.
        assert_eq!(cc.min_height, px(30.0));
        assert_eq!(cc.max_height, px(30.0));
    }

    // ---------- align_child -----------------------------------------------

    #[test]
    fn align_center_places_child_in_the_middle() {
        let node = RenderFractionallySizedBox::new(); // center default
        let offset = node.align_child(
            Size::new(px(100.0), px(80.0)),
            Size::new(px(40.0), px(20.0)),
        );
        assert_eq!(offset, Offset::new(px(30.0), px(30.0)));
    }

    #[test]
    fn align_top_left_places_child_at_origin() {
        let node = RenderFractionallySizedBox::new().with_alignment(Alignment::TOP_LEFT);
        let offset = node.align_child(
            Size::new(px(100.0), px(80.0)),
            Size::new(px(40.0), px(20.0)),
        );
        assert_eq!(offset, Offset::ZERO);
    }

    #[test]
    fn align_bottom_right_places_child_at_full_offset() {
        let node = RenderFractionallySizedBox::new().with_alignment(Alignment::BOTTOM_RIGHT);
        let offset = node.align_child(
            Size::new(px(100.0), px(80.0)),
            Size::new(px(40.0), px(20.0)),
        );
        assert_eq!(offset, Offset::new(px(60.0), px(60.0)));
    }

    // ---------- dry layout ------------------------------------------------

    #[test]
    fn dry_layout_with_full_factors_picks_box_size() {
        let node = RenderFractionallySizedBox::new()
            .with_width_factor(FractionFactor::FULL)
            .with_height_factor(FractionFactor::FULL);
        let size = crate::context::intrinsics_test_support::leaf_dry_layout(|ctx| {
            node.compute_dry_layout(bc(0.0, 200.0, 0.0, 100.0), ctx)
        });
        assert_eq!(size, Size::new(px(200.0), px(100.0)));
    }

    #[test]
    fn dry_layout_zero_factor_collapses_axis() {
        let node = RenderFractionallySizedBox::new().with_width_factor(FractionFactor::ZERO);
        let size = crate::context::intrinsics_test_support::leaf_dry_layout(|ctx| {
            node.compute_dry_layout(bc(0.0, 200.0, 0.0, 100.0), ctx)
        });
        assert_eq!(size.width, px(0.0));
    }

    // ---------- setters ---------------------------------------------------

    #[test]
    fn setters_return_change_flag() {
        let mut node = RenderFractionallySizedBox::default();
        assert!(node.set_width_factor(Some(FractionFactor::HALF)));
        assert!(!node.set_width_factor(Some(FractionFactor::HALF)));
        assert!(node.set_alignment(Alignment::TOP_LEFT));
        assert!(!node.set_alignment(Alignment::TOP_LEFT));
    }
}
