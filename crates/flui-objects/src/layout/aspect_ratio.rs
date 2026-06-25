//! `RenderAspectRatio` — sizes the child to a target width:height ratio.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderAspectRatio`](https://api.flutter.dev/flutter/rendering/RenderAspectRatio-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`). The sizing
//! algorithm follows Flutter's `_applyAspectRatio` exactly: width-first
//! resolution, biased toward inflexibility by checking tighter bounds first.
//!
//! # Rust-native improvements
//!
//! * The ratio is wrapped in [`AspectRatio`] — a validated newtype that
//!   cannot represent a non-positive or non-finite value. Flutter's
//!   `double aspectRatio` field can hold `NaN` and would silently produce
//!   `NaN`-sized layouts; in this port that mistake is unrepresentable.
//! * Constraint queries (`hasBoundedWidth`/`hasBoundedHeight`) are typed
//!   methods on [`BoxConstraints`] returning real `bool`s; Flutter uses
//!   `isFinite` checks on raw doubles.

use flui_tree::Single;
use flui_types::{Offset, Pixels, Size, geometry::px};

use flui_rendering::{
    constraints::{BoxConstraints, Constraints},
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::RenderBox,
};

/// A validated, positive, finite width-to-height ratio.
///
/// `AspectRatio::new` returns `None` for non-positive or non-finite values.
/// Use [`AspectRatio::new_unchecked`] in `const` contexts when the input is
/// known to be valid (`assert!`-guarded panic on debug builds).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AspectRatio(f32);

impl AspectRatio {
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
    pub fn new(value: f32) -> Option<Self> {
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
    pub const fn new_unchecked(value: f32) -> Self {
        debug_assert!(value.is_finite() && value > 0.0, "invalid aspect ratio");
        Self(value)
    }

    /// Creates an aspect ratio from `width / height` of a [`Size`].
    ///
    /// Returns `None` if either dimension is non-positive or the resulting
    /// quotient is not finite.
    pub fn from_size(size: Size) -> Option<Self> {
        let w = size.width.get();
        let h = size.height.get();
        if w > 0.0 && h > 0.0 {
            Self::new(w / h)
        } else {
            None
        }
    }

    /// Returns the underlying `width / height` quotient.
    #[inline]
    #[must_use]
    pub const fn value(self) -> f32 {
        self.0
    }

    /// Returns the inverse `height / width` quotient.
    #[inline]
    #[must_use]
    pub fn inverse(self) -> Self {
        Self(1.0 / self.0)
    }
}

impl Default for AspectRatio {
    fn default() -> Self {
        Self::SQUARE
    }
}

impl From<AspectRatio> for f32 {
    fn from(value: AspectRatio) -> Self {
        value.0
    }
}

/// A render object that forces its child to a specific aspect ratio.
///
/// The algorithm matches Flutter's `RenderAspectRatio` step-for-step:
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
/// At least one of `max_width` / `max_height` must be bounded. With both
/// unbounded, the layout is undefined (there is no finite size that
/// satisfies the ratio); the box falls back to `Size::ZERO` and emits a
/// `tracing::warn!`, matching the Flutter debug-mode assertion in spirit.
///
/// # Example
///
/// ```ignore
/// use flui_objects::{AspectRatio, RenderAspectRatio};
///
/// // 16:9 video frame.
/// let _node = RenderAspectRatio::new(AspectRatio::WIDESCREEN_16_9);
/// ```
#[derive(Debug, Clone)]
pub struct RenderAspectRatio {
    aspect_ratio: AspectRatio,
    has_child: bool,
}

impl RenderAspectRatio {
    /// Creates a render object with the given aspect ratio.
    pub fn new(aspect_ratio: AspectRatio) -> Self {
        Self {
            aspect_ratio,
            has_child: false,
        }
    }

    /// Returns the current aspect ratio.
    #[inline]
    pub fn aspect_ratio(&self) -> AspectRatio {
        self.aspect_ratio
    }

    /// Replaces the aspect ratio; returns true if the value changed.
    pub fn set_aspect_ratio(&mut self, aspect_ratio: AspectRatio) -> bool {
        if self.aspect_ratio == aspect_ratio {
            return false;
        }
        self.aspect_ratio = aspect_ratio;
        true
    }

    /// Computes the size implied by the aspect ratio for the given
    /// constraints, following Flutter's `_applyAspectRatio` exactly.
    fn apply_aspect_ratio(&self, constraints: BoxConstraints) -> Size {
        // Flutter asserts at least one dimension is bounded.
        if !constraints.has_bounded_width() && !constraints.has_bounded_height() {
            tracing::warn!(
                ratio = self.aspect_ratio.value(),
                "RenderAspectRatio: both width and height are unbounded; \
                 falling back to Size::ZERO"
            );
            return Size::ZERO;
        }

        // Tight constraints — the size is fully determined; the ratio is
        // honoured by the parent before we get here.
        if constraints.is_tight() {
            return constraints.smallest();
        }

        let ratio = self.aspect_ratio.value();

        let mut width = constraints.max_width;
        let mut height: Pixels;

        if width.get().is_finite() {
            height = px(width.get() / ratio);
        } else {
            height = constraints.max_height;
            width = px(height.get() * ratio);
        }

        // Bias toward inflexibility: check tighter bounds first.
        if width > constraints.max_width {
            width = constraints.max_width;
            height = px(width.get() / ratio);
        }
        if height > constraints.max_height {
            height = constraints.max_height;
            width = px(height.get() * ratio);
        }
        if width < constraints.min_width {
            width = constraints.min_width;
            height = px(width.get() / ratio);
        }
        if height < constraints.min_height {
            height = constraints.min_height;
            width = px(height.get() * ratio);
        }

        constraints.constrain(Size::new(width, height))
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
            // Flutter passes tight constraints to the child so it can't escape
            // the aspect-ratio sizing decision.
            let child_constraints = BoxConstraints::tight(target_size);
            let _child_size = ctx.layout_child(0, child_constraints);
            ctx.position_child(0, Offset::ZERO);
        } else {
            self.has_child = false;
        }

        target_size
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        if !ctx.is_within_own_size() {
            return false;
        }
        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }

    // ---- intrinsic dimensions ------------------------------------------

    // Flutter parity: proxy_box.dart `RenderAspectRatio` — a finite
    // extent answers with pure ratio math; an unbounded extent defers
    // to the child's own intrinsic (`child?.get* ?? 0.0`).

    fn compute_min_intrinsic_width(
        &self,
        height: f32,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
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
        height: f32,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
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
        width: f32,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
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
        width: f32,
        ctx: &mut flui_rendering::context::BoxIntrinsicsCtx<'_>,
    ) -> f32 {
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
        // child is laid out tight to this size and never consulted
        // (proxy_box.dart `RenderAspectRatio.computeDryLayout`).
        self.apply_aspect_ratio(constraints)
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: flui_rendering::traits::TextBaseline,
        ctx: &mut flui_rendering::context::BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn bc(min_w: f32, max_w: f32, min_h: f32, max_h: f32) -> BoxConstraints {
        BoxConstraints::new(px(min_w), px(max_w), px(min_h), px(max_h))
    }

    // ---------- AspectRatio newtype ---------------------------------------

    #[test]
    fn new_rejects_invalid() {
        assert!(AspectRatio::new(0.0).is_none());
        assert!(AspectRatio::new(-1.0).is_none());
        assert!(AspectRatio::new(f32::NAN).is_none());
        assert!(AspectRatio::new(f32::INFINITY).is_none());
    }

    #[test]
    fn new_accepts_positive_finite() {
        assert!(AspectRatio::new(1.0).is_some());
        assert!(AspectRatio::new(16.0 / 9.0).is_some());
        assert!(AspectRatio::new(0.0001).is_some());
    }

    #[test]
    fn from_size_handles_zero_or_negative() {
        assert!(AspectRatio::from_size(Size::new(px(0.0), px(100.0))).is_none());
        assert!(AspectRatio::from_size(Size::new(px(100.0), px(0.0))).is_none());
        let ar = AspectRatio::from_size(Size::new(px(100.0), px(50.0))).unwrap();
        assert_eq!(ar.value(), 2.0);
    }

    #[test]
    fn inverse_swaps_w_and_h() {
        let ar = AspectRatio::new(2.0).unwrap();
        assert_eq!(ar.inverse().value(), 0.5);
    }

    #[test]
    fn predefined_ratios_are_valid() {
        assert!(AspectRatio::SQUARE.value() > 0.0);
        assert!(AspectRatio::WIDESCREEN_16_9.value() > 1.0);
        assert!(AspectRatio::STANDARD_4_3.value() > 1.0);
        assert!(AspectRatio::ULTRAWIDE_21_9.value() > 2.0);
    }

    // ---------- _applyAspectRatio (Flutter parity) ------------------------

    #[test]
    fn unbounded_both_dims_falls_back_to_zero() {
        let node = RenderAspectRatio::new(AspectRatio::SQUARE);
        let size = node.apply_aspect_ratio(bc(0.0, f32::INFINITY, 0.0, f32::INFINITY));
        assert_eq!(size, Size::ZERO);
    }

    #[test]
    fn tight_constraints_pass_through_unchanged() {
        let node = RenderAspectRatio::new(AspectRatio::SQUARE);
        let size = node.apply_aspect_ratio(BoxConstraints::tight(Size::new(px(50.0), px(80.0))));
        assert_eq!(size, Size::new(px(50.0), px(80.0)));
    }

    #[test]
    fn width_first_basic_case() {
        // 16:9 ratio with max_width=160, max_height=200.
        // width-first: 160 wide → 90 tall (fits within 200) → return (160, 90).
        let node = RenderAspectRatio::new(AspectRatio::WIDESCREEN_16_9);
        let size = node.apply_aspect_ratio(bc(0.0, 160.0, 0.0, 200.0));
        assert_eq!(size, Size::new(px(160.0), px(90.0)));
    }

    #[test]
    fn height_constraint_kicks_in() {
        // Square ratio with max_width=200, max_height=100.
        // width-first: 200 wide → 200 tall, but 200 > 100 → snap height
        // to 100, width = 100 → (100, 100).
        let node = RenderAspectRatio::new(AspectRatio::SQUARE);
        let size = node.apply_aspect_ratio(bc(0.0, 200.0, 0.0, 100.0));
        assert_eq!(size, Size::new(px(100.0), px(100.0)));
    }

    #[test]
    fn unbounded_width_uses_height_path() {
        // 2:1 ratio, width unbounded, max_height=50.
        // height-first: 50 tall → 100 wide → (100, 50).
        let node = RenderAspectRatio::new(AspectRatio::new(2.0).unwrap());
        let size = node.apply_aspect_ratio(bc(0.0, f32::INFINITY, 0.0, 50.0));
        assert_eq!(size, Size::new(px(100.0), px(50.0)));
    }

    #[test]
    fn min_width_pushes_up() {
        // Square ratio with min_width=50, max_width=200, max_height=300.
        // width-first: 200 wide → 200 tall (fits) → result (200,200). No min
        // push-up needed in this case — let's force a case where width drops.
        // Try ratio=10 (very wide), min_w=50, max_w=20, max_h=100:
        let node = RenderAspectRatio::new(AspectRatio::new(10.0).unwrap());
        let size = node.apply_aspect_ratio(bc(50.0, 200.0, 0.0, 5.0));
        // width-first: 200 → 20 (200/10), but 20 > 5 → height=5, width=50.
        // width=50 satisfies min_width=50 — no further bump.
        assert_eq!(size, Size::new(px(50.0), px(5.0)));
    }

    // ---------- intrinsic dimensions --------------------------------------

    #[test]
    fn intrinsics_multiply_or_divide_by_ratio() {
        let node = RenderAspectRatio::new(AspectRatio::new(2.0).unwrap());
        flui_rendering::context::intrinsics_test_support::leaf_intrinsics(|ctx| {
            // For 2:1, width is 2× height; height is 0.5× width.
            assert_eq!(node.compute_min_intrinsic_width(100.0, ctx), 200.0);
            assert_eq!(node.compute_max_intrinsic_width(100.0, ctx), 200.0);
            assert_eq!(node.compute_min_intrinsic_height(100.0, ctx), 50.0);
            assert_eq!(node.compute_max_intrinsic_height(100.0, ctx), 50.0);
        });
    }

    #[test]
    fn intrinsics_zero_for_infinite_input_without_child() {
        let node = RenderAspectRatio::new(AspectRatio::SQUARE);
        flui_rendering::context::intrinsics_test_support::leaf_intrinsics(|ctx| {
            // Unbounded extent defers to the child; childless → 0.0
            // (proxy_box.dart `child?.getMinIntrinsicWidth ?? 0.0`).
            assert_eq!(node.compute_min_intrinsic_width(f32::INFINITY, ctx), 0.0);
            assert_eq!(node.compute_max_intrinsic_height(f32::INFINITY, ctx), 0.0);
        });
    }

    // ---------- API surface -----------------------------------------------

    #[test]
    fn setter_returns_change_flag() {
        let mut node = RenderAspectRatio::new(AspectRatio::SQUARE);
        assert!(node.set_aspect_ratio(AspectRatio::WIDESCREEN_16_9));
        assert!(!node.set_aspect_ratio(AspectRatio::WIDESCREEN_16_9));
    }
}
