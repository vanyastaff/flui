//! `RenderSliverOpacity` — single-child sliver that applies a uniform
//! alpha to its inner sliver during compositing.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderSliverOpacity`](https://api.flutter.dev/flutter/rendering/RenderSliverOpacity-class.html)
//! (`packages/flutter/lib/src/rendering/sliver.dart` — `_RenderSliverOpacity`
//! / the proxy-sliver variant). Layout is a pure passthrough of the
//! parent's [`crate::constraints::SliverConstraints`] to the child; the alpha is consumed
//! by the compositor via the
//! [`crate::traits::PaintEffectsCapability::paint_alpha`] override.
//!
//! # Rust-native improvements
//!
//! * `opacity` is clamped to `[0, 1]` on construction and `set_opacity`;
//!   the cached `alpha: u8` is recomputed at the boundary so paint-time
//!   code reads it as `Some(u8)` without re-clamping per frame.
//! * Setters return `bool` change-flags for pipeline
//!   `mark_needs_paint` short-circuit.
//! * `always_needs_compositing` opt-in mirrors Flutter's
//!   `RenderProxyBox.alwaysNeedsCompositing` toggle and is honoured by
//!   [`RenderSliverOpacity::needs_compositing`] independent of the
//!   alpha value, useful for animations that want a stable compositing
//!   layer.

use flui_tree::Single;

use crate::{
    constraints::SliverGeometry,
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::{HotReloadCapability, PaintEffectsCapability, RenderSliver, SemanticsCapability},
};

// ============================================================================
// RenderSliverOpacity
// ============================================================================

/// A sliver render object that applies transparency to its single
/// sliver child.
///
/// The `opacity` value ranges from `0.0` (fully transparent) to `1.0`
/// (fully opaque). The compositor reads it via the
/// [`crate::traits::PaintEffectsCapability::paint_alpha`] override;
/// layout is a transparent passthrough.
///
/// # Performance
///
/// When `opacity == 1.0` and `always_needs_compositing == false`, no
/// compositing layer is required and `paint_alpha` returns `None`.
/// For frequently-changing opacity (e.g. fade animations), set
/// `always_needs_compositing = true` to avoid layer-tree churn each
/// frame.
#[derive(Debug, Clone)]
pub struct RenderSliverOpacity {
    /// Opacity in `[0.0, 1.0]`.
    opacity: f32,
    /// Cached alpha as `u8` (0..=255) for efficient layer operations.
    alpha: u8,
    /// When `true`, always report `Some(alpha)` from `paint_alpha`,
    /// even when `alpha == 255`. Useful for stable compositing under
    /// animation.
    always_needs_compositing: bool,
}

impl RenderSliverOpacity {
    /// Creates a sliver-opacity render object with the given opacity
    /// (clamped to `[0, 1]`).
    pub fn new(opacity: f32) -> Self {
        let clamped = opacity.clamp(0.0, 1.0);
        Self {
            opacity: clamped,
            alpha: Self::opacity_to_alpha(clamped),
            always_needs_compositing: false,
        }
    }

    /// Creates a fully-opaque sliver-opacity render object
    /// (`opacity = 1.0`).
    #[must_use]
    pub fn opaque() -> Self {
        Self::new(1.0)
    }

    /// Creates a fully-transparent sliver-opacity render object
    /// (`opacity = 0.0`).
    #[must_use]
    pub fn transparent() -> Self {
        Self::new(0.0)
    }

    /// Returns the current opacity in `[0.0, 1.0]`.
    #[inline]
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Returns the cached alpha (`0..=255`).
    #[inline]
    pub fn alpha(&self) -> u8 {
        self.alpha
    }

    /// Returns whether compositing is needed.
    ///
    /// Returns `true` only when `always_needs_compositing` is set OR the alpha
    /// is non-trivially blended (`0 < alpha < 255`). Fully-transparent
    /// (`alpha == 0`) does not need compositing because the subtree is skipped
    /// entirely — Flutter parity: `alwaysNeedsCompositing => alpha > 0`.
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.always_needs_compositing || (self.alpha > 0 && self.alpha != 255)
    }

    /// Returns the `always_needs_compositing` flag.
    #[inline]
    pub fn always_needs_compositing(&self) -> bool {
        self.always_needs_compositing
    }

    /// Updates the opacity (clamped to `[0, 1]`); returns `true` iff
    /// the resulting clamped value differs from the current one.
    pub fn set_opacity(&mut self, opacity: f32) -> bool {
        let clamped = opacity.clamp(0.0, 1.0);
        if (self.opacity - clamped).abs() <= f32::EPSILON {
            return false;
        }
        self.opacity = clamped;
        self.alpha = Self::opacity_to_alpha(clamped);
        true
    }

    /// Updates the `always_needs_compositing` flag; returns `true` iff
    /// the value changed.
    pub fn set_always_needs_compositing(&mut self, value: bool) -> bool {
        if self.always_needs_compositing == value {
            return false;
        }
        self.always_needs_compositing = value;
        true
    }

    /// Converts opacity (`0.0..=1.0`) to alpha (`0..=255`).
    #[inline]
    fn opacity_to_alpha(opacity: f32) -> u8 {
        (opacity * 255.0).round() as u8
    }
}

impl Default for RenderSliverOpacity {
    /// Defaults to fully-opaque (Flutter parity).
    fn default() -> Self {
        Self::opaque()
    }
}

impl flui_foundation::Diagnosticable for RenderSliverOpacity {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_default_double("opacity", self.opacity, 1.0, None);
        builder.add_flag(
            "always_needs_compositing",
            self.always_needs_compositing,
            "always needs compositing",
        );
    }
}

impl RenderSliver for RenderSliverOpacity {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, SliverPhysicalParentData>,
    ) -> SliverGeometry {
        let constraints = *ctx.constraints();

        if ctx.child_count() > 0 {
            // Transparent passthrough — opacity does not affect layout.
            ctx.layout_child(0, constraints)
        } else {
            SliverGeometry::ZERO
        }
    }

    fn hit_test(
        &self,
        ctx: &mut SliverHitTestContext<'_, Single, SliverPhysicalParentData>,
    ) -> bool {
        // Transparent — fully-transparent slivers still hit-test (Flutter
        // parity: `RenderSliverOpacity` does not gate hit-testing on
        // alpha, leaving that to `RenderSliverIgnorePointer`). The
        // opacity object adds no extra hit area.
        ctx.hit_test_child_at_layout_offset(0)
    }
}

// Mythos Step 11: PaintEffectsCapability override — the whole point of
// RenderSliverOpacity. The pipeline reads paint_alpha through a
// `&dyn RenderObject<SliverProtocol>`; the supertrait chain resolves
// here.
impl PaintEffectsCapability for RenderSliverOpacity {
    fn paint_alpha(&self) -> Option<u8> {
        // None when fully opaque (255) OR fully transparent (0) without the
        // always-needs-compositing flag: neither requires an OpacityLayer.
        // Flutter proxy_sliver.dart: alpha=0 → layer=null (no layer, just skip).
        if (self.alpha == 255 || self.alpha == 0) && !self.always_needs_compositing {
            None
        } else {
            Some(self.alpha)
        }
    }

    fn skip_paint(&self) -> bool {
        // Flutter proxy_sliver.dart: `if (_alpha == 0) { return; }`
        // Fully transparent without the always-compositing flag: suppress child
        // paint entirely (no invisible GPU draws).
        self.alpha == 0 && !self.always_needs_compositing
    }
}

impl SemanticsCapability for RenderSliverOpacity {}
impl HotReloadCapability for RenderSliverOpacity {}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_clamps_to_unit_interval() {
        assert!((RenderSliverOpacity::new(0.5).opacity() - 0.5).abs() < f32::EPSILON);
        assert!((RenderSliverOpacity::new(1.5).opacity() - 1.0).abs() < f32::EPSILON);
        assert!((RenderSliverOpacity::new(-0.5).opacity() - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn alpha_tracks_opacity() {
        let o = RenderSliverOpacity::new(0.5);
        // 0.5 * 255 = 127.5 → round → 128.
        assert_eq!(o.alpha(), 128);
    }

    #[test]
    fn opaque_and_transparent_constructors() {
        let opaque = RenderSliverOpacity::opaque();
        assert_eq!(opaque.alpha(), 255);
        assert!(!opaque.needs_compositing());

        let transparent = RenderSliverOpacity::transparent();
        assert_eq!(transparent.alpha(), 0);
        // Flutter: alwaysNeedsCompositing => alpha > 0, so alpha=0 must NOT
        // need compositing (the subtree is skipped entirely).
        assert!(!transparent.needs_compositing());
    }

    #[test]
    fn default_is_opaque() {
        let o = RenderSliverOpacity::default();
        assert!((o.opacity() - 1.0).abs() < f32::EPSILON);
        assert_eq!(o.alpha(), 255);
    }

    #[test]
    fn set_opacity_returns_change_flag() {
        let mut o = RenderSliverOpacity::new(1.0);
        assert!(!o.set_opacity(1.0)); // no-op
        assert!(o.set_opacity(0.25));
        // 0.25 * 255 = 63.75 → round → 64.
        assert_eq!(o.alpha(), 64);
    }

    #[test]
    fn set_always_needs_compositing_returns_change_flag() {
        let mut o = RenderSliverOpacity::opaque();
        assert!(!o.set_always_needs_compositing(false)); // no-op
        assert!(o.set_always_needs_compositing(true));
        assert!(o.always_needs_compositing());
        assert!(o.needs_compositing()); // forced on even with alpha=255.
    }

    #[test]
    fn paint_alpha_returns_none_when_opaque_without_force() {
        let o = RenderSliverOpacity::opaque();
        assert_eq!(o.paint_alpha(), None);
    }

    // 1.3 RED→GREEN: alpha=0 must return None (no layer), not Some(0).
    // Flutter proxy_sliver.dart: alpha=0 → layer=null (no OpacityLayer emitted).
    // Before fix: returned Some(0). After fix: returns None.
    #[test]
    fn paint_alpha_returns_none_when_transparent() {
        let o = RenderSliverOpacity::transparent(); // alpha = 0
        assert_eq!(
            o.paint_alpha(),
            None,
            "alpha=0 without always-flag must return None (no OpacityLayer); \
             Flutter proxy_sliver.dart: alpha=0 → layer=null"
        );
    }

    #[test]
    fn skip_paint_true_when_transparent() {
        assert!(RenderSliverOpacity::transparent().skip_paint());
        assert!(!RenderSliverOpacity::opaque().skip_paint());
        assert!(!RenderSliverOpacity::new(0.5).skip_paint());
    }

    // alpha=0 WITH always-flag: still needs compositing (forced), so
    // paint_alpha returns Some(0) and skip_paint returns false.
    #[test]
    fn paint_alpha_returns_some_when_transparent_but_forced() {
        let mut o = RenderSliverOpacity::transparent();
        o.set_always_needs_compositing(true);
        assert_eq!(o.paint_alpha(), Some(0));
        assert!(!o.skip_paint());
    }

    #[test]
    fn paint_alpha_returns_some_when_partial() {
        let o = RenderSliverOpacity::new(0.5);
        assert_eq!(o.paint_alpha(), Some(128));
    }

    #[test]
    fn paint_alpha_returns_some_when_forced() {
        let mut o = RenderSliverOpacity::opaque();
        o.set_always_needs_compositing(true);
        assert_eq!(o.paint_alpha(), Some(255));
    }

    #[test]
    fn debug_fill_properties_lists_alpha_and_flags() {
        use flui_foundation::{Diagnosticable, DiagnosticsBuilder};
        let o = RenderSliverOpacity::new(0.5);
        let mut builder = DiagnosticsBuilder::new();
        o.debug_fill_properties(&mut builder);
        let names: Vec<String> = builder
            .build()
            .iter()
            .map(|p| p.name().to_string())
            .collect();
        assert!(
            names.iter().any(|n| n == "opacity"),
            "missing diagnostic field: opacity"
        );
    }
}
