//! `RenderFractionallySizedBox` — sizes the child as a fraction of the
//! parent's available space, and aligns it inside the parent.
//!
//! # Scope
//!
//! Covers the non-overflow case (matching the `FractionallySizedBox` widget
//! contract), with one deliberate rule on unbounded constraints (see
//! *Design*).
//!
//! # Design
//!
//! * Fraction factors are typed via [`FractionFactor`], a newtype that
//!   forbids negative values at the API boundary rather than silently
//!   zeroing them out at runtime.
//! * `width_factor`/`height_factor` are `Option<FractionFactor>` —
//!   `None` means "inherit the parent constraint" without overloading `0.0`
//!   as a magic sentinel.
//! * Alignment uses [`flui_painting::Alignment`] (`x`,`y` ∈ `[-1, 1]`) rather
//!   than the painting-side parallel definition, keeping the alignment
//!   math consistent with `RenderTransform` / `RenderCenter`.
//! * **Unbounded axes:** on a factored axis whose incoming `max` is
//!   unbounded, the child is sized as `parent_min × factor` rather than
//!   `parent_max × factor` (which would be a degenerate infinite child).
//!   See the per-axis note on `child_constraints` and the infinite-`max`
//!   factor tests.

use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Size};
use flui_painting::Alignment;

use flui_rendering::{
    constraints::BoxConstraints,
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// A non-negative fraction used as a sizing factor.
///
/// `0.0` means "collapse this axis", `1.0` means "match the parent",
/// `2.0` would mean "twice the parent" — values above 1.0 are accepted
/// because that's how `FractionallySizedBox` is used in practice with the
/// overflow flag implicit to the parent layer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FractionFactor(f64);

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
    pub fn new(value: f64) -> Option<Self> {
        if value.is_finite() && value >= 0.0 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Creates a fraction factor without validation (debug-asserted).
    #[must_use]
    pub const fn new_unchecked(value: f64) -> Self {
        debug_assert!(value.is_finite() && value >= 0.0, "invalid fraction factor");
        Self(value)
    }

    /// Returns the underlying f64 value.
    #[inline]
    #[must_use]
    pub const fn value(self) -> f64 {
        self.0
    }
}

impl From<FractionFactor> for f64 {
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
/// use flui_objects::{FractionFactor, RenderFractionallySizedBox};
/// use flui_painting::Alignment;
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

    /// Sets the width factor and reports layout when changed.
    pub fn set_width_factor(
        &mut self,
        factor: Option<FractionFactor>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.width_factor == factor {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.width_factor = factor;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Sets the height factor and reports layout when changed.
    pub fn set_height_factor(
        &mut self,
        factor: Option<FractionFactor>,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.height_factor == factor {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.height_factor = factor;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Sets the alignment and reports layout when changed.
    pub fn set_alignment(&mut self, alignment: Alignment) -> flui_rendering::RenderUpdateImpact {
        if self.alignment == alignment {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.alignment = alignment;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Computes the tight constraints to pass to the child for these
    /// incoming constraints.
    /// Width factor as a bare multiplier, `1.0` when unset (as used in the
    /// intrinsic formulas).
    fn width_factor_or_one(&self) -> f64 {
        self.width_factor.map_or(1.0, FractionFactor::value)
    }

    /// Height factor as a bare multiplier, `1.0` when unset.
    fn height_factor_or_one(&self) -> f64 {
        self.height_factor.map_or(1.0, FractionFactor::value)
    }

    fn child_constraints(&self, incoming: BoxConstraints) -> BoxConstraints {
        // Width axis.
        let (min_w, max_w) = match self.width_factor {
            Some(factor) => {
                let base = if incoming.max_width.is_finite() {
                    incoming.max_width
                } else {
                    incoming.min_width
                };
                let target = base * factor.value();
                (target, target)
            }
            None => (incoming.min_width, incoming.max_width),
        };
        // Height axis.
        let (min_h, max_h) = match self.height_factor {
            Some(factor) => {
                let base = if incoming.max_height.is_finite() {
                    incoming.max_height
                } else {
                    incoming.min_height
                };
                let target = base * factor.value();
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
        let dx = free_w * (self.alignment.x + 1.0) * 0.5;
        let dy = free_h * (self.alignment.y + 1.0) * 0.5;
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
                .map_or_else(|| "unset".to_string(), |f| format!("{}", f.value())),
        );
        builder.add(
            "height_factor",
            self.height_factor
                .map_or_else(|| "unset".to_string(), |f| format!("{}", f.value())),
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

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let incoming = *ctx.constraints();

        if ctx.child_count() > 0 {
            self.has_child = true;
            let child_constraints = self.child_constraints(incoming);
            let child_size = ctx.layout_child(0, child_constraints);
            // Our box = the parent's tightest acceptable size that wraps
            // the child. With factors set, the child IS that size; without
            // factors, we use the child as-is.
            let size = incoming.constrain(child_size);
            self.child_offset = self.align_child(size, child_size);
            ctx.position_child(0, self.child_offset);
            size
        } else {
            self.has_child = false;
            // Without a child, our size is determined by the factors alone.
            let computed = self.child_constraints(incoming);
            self.child_offset = Offset::ZERO;
            incoming.constrain(Size::new(computed.min_width, computed.min_height))
        }
    }

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

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut flui_rendering::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        let computed = self.child_constraints(constraints);
        if ctx.child_count() > 0 {
            constraints.constrain(ctx.child_dry_layout(0, computed))
        } else {
            constraints.constrain(Size::new(computed.min_width, computed.min_height))
        }
    }

    // The child is probed at the OTHER axis's scaled extent (infinity
    // absorption keeps an unbounded extent unbounded), and the answer is
    // divided back by this axis's factor.

    fn compute_min_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
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
        height: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
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
        width: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
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
        width: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
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

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: flui_rendering::traits::TextBaseline,
        ctx: &mut flui_rendering::context::BoxDryBaselineCtx<'_>,
    ) -> Option<f64> {
        if ctx.child_count() == 0 {
            return None;
        }
        let child_constraints = self.child_constraints(constraints);
        let child_baseline = ctx.child_dry_baseline(0, child_constraints, baseline)?;
        let child_size = ctx.child_dry_layout(0, child_constraints);
        let size = constraints.constrain(child_size);
        let offset = self.align_child(size, child_size);
        Some(child_baseline + offset.dy)
    }
}

// ===========================================================================
// Tests
// ===========================================================================
