//! `RenderAspectRatio` — sizes the child to a target width:height ratio.
//!
//! # Sizing
//!
//! Width-first resolution, biased toward inflexibility by checking tighter
//! bounds first (see [`RenderAspectRatio`] for the steps).
//!
//! # Design
//!
//! * The ratio is wrapped in [`AspectRatioFactor`] — a validated newtype that
//!   cannot represent a non-positive or non-finite value, so a `NaN` ratio
//!   cannot silently produce `NaN`-sized layouts.
//! * Constraint queries (`has_bounded_width`/`has_bounded_height`) are typed
//!   methods on [`BoxConstraints`] returning real `bool`s.

use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Size};

use flui_rendering::{
    constraints::{BoxConstraints, Constraints},
    context::BoxLayoutContext,
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// A validated, positive, finite width-to-height ratio.
///
/// `AspectRatioFactor::new` returns `None` for non-positive or non-finite values.
/// Use [`AspectRatioFactor::new_unchecked`] in `const` contexts when the input is
/// known to be valid (`assert!`-guarded panic on debug builds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AspectRatioFactor(f64);

impl AspectRatioFactor {
    /// Square (1:1).
    pub const SQUARE: Self = Self(1.0);

    /// 16:9 — common video / widescreen ratio.
    pub const WIDESCREEN_16_9: Self = Self(16.0 / 9.0);

    /// 4:3 — classic TV / camera ratio.
    pub const STANDARD_4_3: Self = Self(4.0 / 3.0);

    /// 3:2 — common photography ratio.
    pub const PHOTO_3_2: Self = Self(3.0 / 2.0);

    /// 21:9 — ultra-widescreen.
    pub const ULTRAWIDE_21_9: Self = Self(21.0 / 9.0);

    /// Creates an aspect ratio from a `width / height` quotient.
    ///
    /// Returns `None` if the value is `<= 0`, NaN, or infinite.
    #[must_use]
    pub fn new(value: f64) -> Option<Self> {
        if value.is_finite() && value > 0.0 {
            Some(Self(value))
        } else {
            None
        }
    }

    /// Creates an aspect ratio without validation.
    ///
    /// # Panics
    ///
    /// In debug builds, panics if `value` is non-positive, NaN, or infinite.
    /// In release builds the value is stored as-is (use this only when the
    /// input is a compile-time literal that has been visually validated).
    #[must_use]
    pub const fn new_unchecked(value: f64) -> Self {
        debug_assert!(value.is_finite() && value > 0.0, "invalid aspect ratio");
        Self(value)
    }

    /// Creates an aspect ratio from `width / height` of a [`Size`].
    ///
    /// Returns `None` if either dimension is non-positive or the resulting
    /// quotient is not finite.
    pub fn from_size(size: Size) -> Option<Self> {
        let w = size.width;
        let h = size.height;
        if w > 0.0 && h > 0.0 {
            Self::new(w / h)
        } else {
            None
        }
    }

    /// Returns the underlying `width / height` quotient.
    #[inline]
    #[must_use]
    pub const fn value(self) -> f64 {
        self.0
    }

    /// Returns the inverse `height / width` quotient.
    #[inline]
    #[must_use]
    pub fn inverse(self) -> Self {
        Self(1.0 / self.0)
    }
}

impl Default for AspectRatioFactor {
    fn default() -> Self {
        Self::SQUARE
    }
}

impl From<AspectRatioFactor> for f64 {
    fn from(value: AspectRatioFactor) -> Self {
        value.0
    }
}

/// A render object that forces its child to a specific aspect ratio.
///
/// The algorithm:
/// 1. Default to width = `constraints.max_width`, height = width / ratio.
/// 2. If width is unbounded, swap: take height = `constraints.max_height`
///    and compute width = height × ratio.
/// 3. Clamp width down if it exceeds max_width, recomputing height.
/// 4. Clamp height down if it exceeds max_height, recomputing width.
/// 5. Clamp width up if it falls below min_width, recomputing height.
/// 6. Clamp height up if it falls below min_height, recomputing width.
/// 7. Finally, [`BoxConstraints::constrain`] the result.
///
/// The order is intentional: tighter bounds win over looser ones.
///
/// # Constraints requirement
///
/// With both maxima unbounded, the box chooses the smallest ratio-preserving
/// size that covers the minimum dimensions. Zero minima therefore yield zero
/// size. An unbounded axis still has a representable upper limit of `f64::MAX`:
/// if the ratio cannot fit the minimum dimensions within that limit, finite
/// geometry and the parent's constraints take precedence over the ratio.
/// This finite sizing contract requires normalized constraints with finite
/// minimum dimensions. Infinite minima cannot admit finite geometry; their
/// minimum size is returned for the pipeline to reject as invalid geometry.
///
/// # Example
///
/// ```ignore
/// use flui_objects::{AspectRatioFactor, RenderAspectRatio};
///
/// // 16:9 video frame.
/// let _node = RenderAspectRatio::new(AspectRatioFactor::WIDESCREEN_16_9);
/// ```
#[derive(Debug, Clone)]
pub struct RenderAspectRatio {
    aspect_ratio: AspectRatioFactor,
    has_child: bool,
}

impl RenderAspectRatio {
    /// Creates a render object with the given aspect ratio.
    pub fn new(aspect_ratio: AspectRatioFactor) -> Self {
        Self {
            aspect_ratio,
            has_child: false,
        }
    }

    /// Returns the current aspect ratio.
    #[inline]
    pub fn aspect_ratio(&self) -> AspectRatioFactor {
        self.aspect_ratio
    }

    /// Replaces the aspect ratio and reports layout when changed.
    pub fn set_aspect_ratio(
        &mut self,
        aspect_ratio: AspectRatioFactor,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.aspect_ratio == aspect_ratio {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.aspect_ratio = aspect_ratio;
        flui_rendering::RenderUpdateImpact::LAYOUT
    }

    /// Computes the size implied by the aspect ratio for the given
    /// constraints.
    fn apply_aspect_ratio(&self, constraints: BoxConstraints) -> Size {
        let ratio = self.aspect_ratio.value();
        if !ratio.is_finite() || ratio <= 0.0 {
            tracing::warn!(
                ratio,
                "RenderAspectRatio: invalid ratio; using minimum size"
            );
            return constraints.smallest();
        }

        // Normalized constraints can still force an infinite dimension. No
        // finite answer satisfies them; preserve the minimum for the pipeline's
        // typed geometry rejection instead of constructing an inverted range.
        if !constraints.min_width.is_finite() || !constraints.min_height.is_finite() {
            return constraints.smallest();
        }

        // Infinity admits any finite length, but not an infinite layout output.
        // Clamp intermediate overflow against the representable range as well
        // as the parent's bounds before returning geometry.
        let finite_constraints = BoxConstraints::new(
            constraints.min_width,
            constraints.max_width.min(f64::MAX),
            constraints.min_height,
            constraints.max_height.min(f64::MAX),
        );
        if !constraints.has_bounded_width() && !constraints.has_bounded_height() {
            let mut width = constraints.min_width;
            let mut height = width / ratio;
            if height < constraints.min_height {
                height = constraints.min_height;
                width = height * ratio;
            }
            return finite_constraints.constrain(Size::new(width, height));
        }

        // Tight constraints — the size is fully determined; the ratio is
        // honoured by the parent before we get here.
        if constraints.is_tight() {
            return constraints.smallest();
        }

        let mut width = constraints.max_width;
        let mut height: f64;

        if width.is_finite() {
            height = width / ratio;
        } else {
            height = constraints.max_height;
            width = height * ratio;
        }

        // Bias toward inflexibility: check tighter bounds first.
        if width > finite_constraints.max_width {
            width = finite_constraints.max_width;
            height = width / ratio;
        }
        if height > finite_constraints.max_height {
            height = finite_constraints.max_height;
            width = height * ratio;
        }
        if width < constraints.min_width {
            width = constraints.min_width;
            height = width / ratio;
        }
        if height < constraints.min_height {
            height = constraints.min_height;
            width = height * ratio;
        }

        finite_constraints.constrain(Size::new(width, height))
    }
}

impl flui_foundation::Diagnosticable for RenderAspectRatio {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_double("aspect_ratio", self.aspect_ratio.value(), None);
    }
}

impl RenderBox for RenderAspectRatio {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let incoming = *ctx.constraints();
        let target_size = self.apply_aspect_ratio(incoming);

        if ctx.child_count() > 0 {
            self.has_child = true;
            // The child gets tight constraints so it can't escape the
            // aspect-ratio sizing decision.
            let child_constraints = BoxConstraints::tight(target_size);
            let _child_size = ctx.layout_child(0, child_constraints);
            ctx.position_child(0, Offset::ZERO);
        } else {
            self.has_child = false;
        }

        target_size
    }

    flui_rendering::forward_single_child_box_hit_test!();

    // ---- intrinsic dimensions ------------------------------------------

    // A finite extent answers with pure ratio math; an unbounded extent
    // defers to the child's own intrinsic (0 with no child).

    fn compute_min_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        if height.is_finite() {
            return height * self.aspect_ratio.value();
        }
        if ctx.child_count() > 0 {
            ctx.child_min_intrinsic_width(0, height)
        } else {
            0.0
        }
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        if height.is_finite() {
            return height * self.aspect_ratio.value();
        }
        if ctx.child_count() > 0 {
            ctx.child_max_intrinsic_width(0, height)
        } else {
            0.0
        }
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        if width.is_finite() {
            return width / self.aspect_ratio.value();
        }
        if ctx.child_count() > 0 {
            ctx.child_min_intrinsic_height(0, width)
        } else {
            0.0
        }
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f64 {
        if width.is_finite() {
            return width / self.aspect_ratio.value();
        }
        if ctx.child_count() > 0 {
            ctx.child_max_intrinsic_height(0, width)
        } else {
            0.0
        }
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        _ctx: &mut flui_rendering::context::BoxDryLayoutCtx<'_>,
    ) -> Size {
        // Sizing is fully determined by the ratio + constraints; the
        // child is laid out tight to this size and never consulted.
        self.apply_aspect_ratio(constraints)
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
        let tight = BoxConstraints::tight(self.apply_aspect_ratio(constraints));
        ctx.child_dry_baseline(0, tight, baseline)
    }
}

// ===========================================================================
// Tests
// ===========================================================================
