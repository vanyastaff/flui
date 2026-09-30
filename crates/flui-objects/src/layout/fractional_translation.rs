//! `RenderFractionalTranslation` — single-child proxy that, at paint
//! time, shifts its child by a fraction of the child's own size.
//!
//! # Design
//!
//! Carrying the *fraction* in an `Offset` (a `dx, dy` of *pixels*) would be
//! a unit mismatch — a pixels-typed value holding a fraction — enforced only
//! by convention.
//!
//! A dedicated [`TranslationFraction`] newtype makes
//! "fraction of child size" visible in the API surface. Lengths
//! never appear in the translation slot; the conversion happens once
//! inside `paint`/`hit_test` against the driver-supplied size (from
//! `RenderState`). The intent collapses into the type system instead of
//! the docstring.

use flui_foundation::Single;
use flui_foundation::geometry::Lerp;
use flui_foundation::geometry::{Matrix4, Offset, Size};

use flui_rendering::{context::BoxHitTestContext, parent_data::BoxParentData, traits::RenderBox};

// =============================================================================
// TranslationFraction — typed fraction-of-size translation
// =============================================================================

/// A 2D translation expressed as fractions of the translated subject's
/// own size.
///
/// `TranslationFraction { dx: -0.5, dy: 0.0 }` shifts the subject left
/// by half its own width; `{ dx: 1.0, dy: 0.0 }` shifts it right by
/// its full width (off-stage). The fractions are unit-less `f64`,
/// not pixels — distinguishing them from `Offset`, which carries
/// logical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TranslationFraction {
    /// Horizontal fraction (multiplied by `size.width` at use site).
    pub dx: f64,
    /// Vertical fraction (multiplied by `size.height` at use site).
    pub dy: f64,
}

impl TranslationFraction {
    /// The identity translation (zero on both axes).
    pub const ZERO: Self = Self { dx: 0.0, dy: 0.0 };

    /// Creates a new fractional offset.
    #[inline]
    #[must_use]
    pub const fn new(dx: f64, dy: f64) -> Self {
        Self { dx, dy }
    }

    /// Resolves this fraction against a concrete `size`, producing a
    /// logical-pixel [`Offset`] suitable for canvas math.
    #[inline]
    #[must_use]
    pub fn resolve(&self, size: Size) -> Offset {
        Offset::new(size.width * self.dx, size.height * self.dy)
    }
}

impl Lerp for TranslationFraction {
    /// Component-wise linear interpolation — the fraction itself is a plain
    /// unitless `f64` pair, so this is the same `a + (b - a) * t` every other
    /// `Lerp` scalar uses. Lets an `Animation<TranslationFraction>` (e.g.
    /// `flui-widgets`' `SlideTransition`) drive a `Tween<TranslationFraction>`
    /// directly instead of animating pixel-typed offsets and dividing back
    /// out by a size that may not be known yet.
    #[inline]
    fn lerp_to(&self, other: &Self, t: f64) -> Self {
        Self {
            dx: self.dx + (other.dx - self.dx) * t,
            dy: self.dy + (other.dy - self.dy) * t,
        }
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
    /// pointers land where the user *sees* the child. Defaults to true.
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
    /// `transform_hit_tests = true` (the default).
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

    /// Updates the translation and reports its paint and semantics impact.
    pub fn set_translation(
        &mut self,
        translation: TranslationFraction,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.translation == translation {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.translation = translation;
        flui_rendering::RenderUpdateImpact::PAINT | flui_rendering::RenderUpdateImpact::SEMANTICS
    }

    /// Updates the hit-test transform flag, which schedules no pipeline pass.
    pub fn set_transform_hit_tests(&mut self, value: bool) -> flui_rendering::RenderUpdateImpact {
        if self.transform_hit_tests == value {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.transform_hit_tests = value;
        flui_rendering::RenderUpdateImpact::NONE
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

    flui_rendering::forward_single_child_box_layout!();

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

    /// Translates by `translation.dx * size.width, translation.dy *
    /// size.height`.
    ///
    /// **The default would be wrong here.** `perform_layout` positions the child
    /// at `Offset::ZERO` and `paint` shifts it with `paint_child_at` — an
    /// `offset_override`. The child's *committed* offset, which the default
    /// composes, is zero and says nothing about where the child paints.
    ///
    /// Unlike `hit_test`, this ignores `transform_hit_tests`: it is
    /// unconditional, because it answers "where does the child paint", not
    /// "where can it be hit".
    fn apply_paint_transform(
        &self,
        _child: usize,
        _child_offset: Offset,
        size: Size,
        transform: &mut Matrix4,
    ) {
        let offset = self.pixel_offset(size);
        *transform *= Matrix4::translation(offset.dx, offset.dy, 0.0);
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // Skip the own-bounds check and go straight to the child, so a
        // pointer over the SHIFTED child still hits even when it lies outside
        // the box's original bounds. (RenderTransform does the same.)
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
