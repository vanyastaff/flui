//! `RenderFractionalTranslation` — single-child proxy that, at paint
//! time, shifts its child by a fraction of the child's own size.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderFractionalTranslation`](https://api.flutter.dev/flutter/rendering/RenderFractionalTranslation-class.html)
//! (`packages/flutter/lib/src/rendering/proxy_box.dart`).
//!
//! # Rust-native improvement
//!
//! Flutter overloads `Offset` (a `dx, dy` of *pixels*) to carry the
//! *fraction*: callers write `translation: Offset(-0.5, 0.0)` and the
//! render object multiplies by child size at paint time. The unit
//! mismatch — pixels-typed value holding a fraction — is a runtime
//! convention with no compile-side enforcement.
//!
//! This port introduces a dedicated [`TranslationFraction`] newtype so
//! "fraction of child size" is visible in the API surface. Pixels
//! never appear in the translation slot; the conversion happens once
//! inside `paint`/`hit_test` against the driver-supplied size (from
//! `RenderState`). The intent collapses into the type system instead of
//! the docstring.

use flui_tree::Single;
use flui_types::{Offset, Size, geometry::px};

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::RenderBox,
};

// =============================================================================
// TranslationFraction — typed fraction-of-size translation
// =============================================================================

/// A 2D translation expressed as fractions of the translated subject's
/// own size.
///
/// `TranslationFraction { dx: -0.5, dy: 0.0 }` shifts the subject left
/// by half its own width; `{ dx: 1.0, dy: 0.0 }` shifts it right by
/// its full width (off-stage). The fractions are unit-less `f32`,
/// not pixels — distinguishing them from `Offset` which carries
/// concrete `Pixels`.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TranslationFraction {
    /// Horizontal fraction (multiplied by `size.width` at use site).
    pub dx: f32,
    /// Vertical fraction (multiplied by `size.height` at use site).
    pub dy: f32,
}

impl TranslationFraction {
    /// The identity translation (zero on both axes).
    pub const ZERO: Self = Self { dx: 0.0, dy: 0.0 };

    /// Creates a new fractional offset.
    #[inline]
    #[must_use]
    pub const fn new(dx: f32, dy: f32) -> Self {
        Self { dx, dy }
    }

    /// Resolves this fraction against a concrete `size`, producing a
    /// `Pixels`-typed [`Offset`] suitable for canvas math.
    #[inline]
    #[must_use]
    pub fn resolve(&self, size: Size) -> Offset {
        Offset::new(
            px(size.width.get() * self.dx),
            px(size.height.get() * self.dy),
        )
    }
}

// =============================================================================
// RenderFractionalTranslation
// =============================================================================

/// A render object that translates its child at paint time by
/// [`TranslationFraction`] × child-size.
///
/// Layout passes through untouched (the box adopts the child's size);
/// only paint and (optionally) hit-test apply the translation.
#[derive(Debug, Clone)]
pub struct RenderFractionalTranslation {
    translation: TranslationFraction,
    /// When true, the translation is also applied to hit testing so
    /// pointers land where the user *sees* the child. Flutter parity:
    /// default true.
    transform_hit_tests: bool,
    has_child: bool,
}

impl RenderFractionalTranslation {
    /// Creates a fractional-translation render object.
    pub const fn new(translation: TranslationFraction, transform_hit_tests: bool) -> Self {
        Self {
            translation,
            transform_hit_tests,
            has_child: false,
        }
    }

    /// Creates a fractional-translation render object with
    /// `transform_hit_tests = true` (Flutter parity default).
    pub const fn translated(translation: TranslationFraction) -> Self {
        Self::new(translation, true)
    }

    /// Returns the current fractional translation.
    #[inline]
    pub fn translation(&self) -> TranslationFraction {
        self.translation
    }

    /// Returns whether hit-tests are transformed alongside paint.
    #[inline]
    pub fn transform_hit_tests(&self) -> bool {
        self.transform_hit_tests
    }

    /// Updates the translation; returns true if the value changed.
    pub fn set_translation(&mut self, translation: TranslationFraction) -> bool {
        if self.translation == translation {
            return false;
        }
        self.translation = translation;
        true
    }

    /// Updates the hit-test transform flag; returns true if changed.
    pub fn set_transform_hit_tests(&mut self, value: bool) -> bool {
        if self.transform_hit_tests == value {
            return false;
        }
        self.transform_hit_tests = value;
        true
    }

    /// Resolved pixel offset for the given laid-out size.
    #[inline]
    fn pixel_offset(&self, size: Size) -> Offset {
        self.translation.resolve(size)
    }
}

impl Default for RenderFractionalTranslation {
    fn default() -> Self {
        Self::new(TranslationFraction::ZERO, true)
    }
}

impl flui_foundation::Diagnosticable for RenderFractionalTranslation {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add(
            "translation",
            format!("({}, {})", self.translation.dx, self.translation.dy),
        );
        builder.add_flag(
            "transform_hit_tests",
            self.transform_hit_tests,
            "transform hit tests",
        );
    }
}

impl RenderBox for RenderFractionalTranslation {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
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

    flui_rendering::forward_single_child_box_queries!();

    fn paint(&self, ctx: &mut flui_rendering::context::PaintCx<'_, Single>) {
        if !self.has_child {
            return;
        }
        // `paint_child_at` REPLACES the child's laid-out offset; the
        // child is laid out at the origin here, so the override IS the
        // pixel translation.
        ctx.paint_child_at(self.pixel_offset(ctx.size()));
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // Flutter RenderFractionalTranslation overrides `hitTest` to skip the
        // own-bounds check and delegate straight to `hitTestChildren`
        // (proxy_box.dart), so a pointer over the SHIFTED child still hits even
        // when it lies outside the box's original bounds. (RenderTransform does
        // the same; the prior `is_within_own_size` gate here rejected those hits.)
        if !self.has_child {
            return false;
        }
        if self.transform_hit_tests {
            // The visual content is shifted by `pixel_offset()`; record this
            // offset in the transform stack before testing the child.
            let offset = self.pixel_offset(ctx.own_size());
            ctx.push_offset(offset);
            let child_position =
                Offset::new(ctx.position().dx - offset.dx, ctx.position().dy - offset.dy);
            let hit = ctx.hit_test_child(0, child_position);
            ctx.pop_transform();
            hit
        } else {
            // No transform: test at child's layout offset only
            ctx.hit_test_child_at_layout_offset(0)
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use flui_types::geometry::px;

    use super::*;

    // ---------- TranslationFraction ------------------------------------------

    #[test]
    fn fractional_offset_zero_resolves_to_zero() {
        let off = TranslationFraction::ZERO;
        assert_eq!(
            off.resolve(Size::new(px(200.0), px(100.0))),
            Offset::new(px(0.0), px(0.0))
        );
    }

    #[test]
    fn fractional_offset_resolves_to_fraction_of_size() {
        let off = TranslationFraction::new(-0.5, 0.25);
        let r = off.resolve(Size::new(px(200.0), px(100.0)));
        assert_eq!(r.dx, px(-100.0));
        assert_eq!(r.dy, px(25.0));
    }

    #[test]
    fn fractional_offset_one_shifts_by_full_size() {
        let off = TranslationFraction::new(1.0, 1.0);
        let r = off.resolve(Size::new(px(80.0), px(40.0)));
        assert_eq!(r, Offset::new(px(80.0), px(40.0)));
    }

    // ---------- RenderFractionalTranslation -------------------------------

    #[test]
    fn defaults_have_zero_translation_and_transform_hit_tests() {
        let node = RenderFractionalTranslation::default();
        assert_eq!(node.translation(), TranslationFraction::ZERO);
        assert!(node.transform_hit_tests());
    }

    #[test]
    fn translated_helper_defaults_transform_hit_tests_to_true() {
        let node = RenderFractionalTranslation::translated(TranslationFraction::new(-0.5, 0.0));
        assert_eq!(node.translation(), TranslationFraction::new(-0.5, 0.0));
        assert!(node.transform_hit_tests());
    }

    #[test]
    fn new_round_trips_both_fields() {
        let node = RenderFractionalTranslation::new(TranslationFraction::new(0.25, 0.5), false);
        assert_eq!(node.translation(), TranslationFraction::new(0.25, 0.5));
        assert!(!node.transform_hit_tests());
    }

    #[test]
    fn pixel_offset_multiplies_injected_size_by_fraction() {
        // The pixel translation resolves the fraction against the
        // laid-out size the pipeline hands in (RenderState via `ctx.size()`
        // / `ctx.own_size()`), not a cached field: -0.5 × 200 = -100,
        // 0.25 × 100 = 25.
        let node = RenderFractionalTranslation::translated(TranslationFraction::new(-0.5, 0.25));
        assert_eq!(
            node.pixel_offset(Size::new(px(200.0), px(100.0))),
            Offset::new(px(-100.0), px(25.0)),
        );
    }

    #[test]
    fn setters_return_change_flag() {
        let mut node = RenderFractionalTranslation::default();
        assert!(node.set_translation(TranslationFraction::new(0.1, 0.2)));
        assert!(!node.set_translation(TranslationFraction::new(0.1, 0.2)));
        assert!(node.set_transform_hit_tests(false));
        assert!(!node.set_transform_hit_tests(false));
    }

    #[test]
    fn debug_fill_properties_lists_state() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let node = RenderFractionalTranslation::default();
        let mut builder = DiagnosticsBuilder::new();
        node.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        for required in ["translation", "transform_hit_tests"] {
            assert!(
                names.iter().any(|n| n == required),
                "missing diagnostic field: {required}"
            );
        }
    }
}
