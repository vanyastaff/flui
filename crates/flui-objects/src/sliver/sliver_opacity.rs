//! `RenderSliverOpacity` — single-child sliver that applies a uniform
//! alpha to its inner sliver during compositing.
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's
//! [`RenderSliverOpacity`](https://api.flutter.dev/flutter/rendering/RenderSliverOpacity-class.html)
//! (`packages/flutter/lib/src/rendering/sliver.dart` — `_RenderSliverOpacity`
//! / the proxy-sliver variant). Layout is a pure passthrough of the
//! parent's [`flui_rendering::constraints::SliverConstraints`] to the child; the alpha is consumed
//! by the compositor via the
//! [`flui_rendering::traits::RenderSliver::paint_alpha`] override.
//!
//! # Rust-native improvements
//!
//! * `opacity` is clamped to `[0, 1]` on construction and `set_opacity`;
//!   the cached `alpha: u8` is recomputed at the boundary so paint-time
//!   code reads it as `Some(u8)` without re-clamping per frame.
//! * The opacity setter reports the exact paint, compositing, and semantics
//!   impact of the alpha transition.
//! * `always_needs_compositing` opt-in mirrors Flutter's
//!   `RenderProxyBox.alwaysNeedsCompositing` toggle and is honoured by
//!   [`RenderSliverOpacity::needs_compositing`] independent of the
//!   alpha value, useful for animations that want a stable compositing
//!   layer.

use flui_tree::Single;

use flui_rendering::{
    constraints::SliverGeometry,
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    traits::RenderSliver,
};

// ============================================================================
// RenderSliverOpacity
// ============================================================================

/// A sliver render object that applies transparency to its single
/// sliver child.
///
/// The `opacity` value ranges from `0.0` (fully transparent) to `1.0`
/// (fully opaque). The compositor reads it via the
/// [`flui_rendering::traits::RenderSliver::paint_alpha`] override;
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
    /// entirely.
    ///
    /// **Recorded divergence, not parity.** Upstream's predicate is
    /// `child != null && _alpha > 0`, which is TRUE at alpha 255 — its own
    /// `proxy_sliver_test.dart` case "RenderSliverOpacity does composite if it
    /// is opaque" asserts exactly that, and this predicate does not satisfy it.
    /// The `alpha != 255` term is deliberate and is the same `is_layered`
    /// threshold `paint_alpha` and `skip_paint` already use: at alpha 255 no
    /// layer is ever allocated (`paint_alpha` returns `None`), so demanding
    /// compositing there is pure overhead with no visual effect. The full
    /// rationale is recorded once, on the sibling that first made the call —
    /// see `proxy::animated_opacity`'s `is_repaint_boundary` comment. Oracle
    /// for the divergent value: `opaque_and_transparent_constructors` below.
    #[inline]
    pub fn needs_compositing(&self) -> bool {
        self.always_needs_compositing || (self.alpha > 0 && self.alpha != 255)
    }

    /// Returns the raw `always_needs_compositing` field (the explicit opt-in flag).
    ///
    /// This is the stored boolean flag only. Use
    /// [`needs_compositing`](Self::needs_compositing) to test whether
    /// compositing is needed for any reason (field OR alpha in `(0, 255)`).
    #[inline]
    pub fn always_needs_compositing_flag(&self) -> bool {
        self.always_needs_compositing
    }

    /// Updates the opacity (clamped to `[0, 1]`) and reports its exact pipeline
    /// impact.
    pub fn set_opacity(&mut self, opacity: f32) -> flui_rendering::RenderUpdateImpact {
        let clamped = opacity.clamp(0.0, 1.0);
        if (self.opacity - clamped).abs() <= f32::EPSILON {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        let old_needs_compositing = self.needs_compositing();
        let old_is_visible = self.alpha > 0;
        // Whether this node suppresses its subtree's paint entirely. Read
        // through the trait method rather than re-deriving `alpha == 0`, so
        // this stays correct if that predicate changes.
        let old_skips_paint = <Self as RenderSliver>::skip_paint(self);
        self.opacity = clamped;
        self.alpha = Self::opacity_to_alpha(clamped);
        // A pure alpha change lands ONLY on the `OpacityLayer` this node
        // pushes, so the frame can rebuild that layer and replay the enclosing
        // repaint boundary's retained output rather than repainting the
        // subtree.
        //
        // DIVERGENCE FROM THE REFERENCE, and a deliberate improvement — see
        // `flui-rendering/ARCHITECTURE.md`, "A composited-layer update patches
        // the enclosing capture". Flutter's `RenderSliverOpacity.opacity`
        // setter calls `markNeedsPaint()` (`proxy_sliver.dart`), NOT
        // `markNeedsCompositedLayerUpdate()` — and it has no choice: that
        // mechanism requires the node to BE a repaint boundary, and
        // `RenderSliverOpacity` never overrides `isRepaintBoundary` — the word
        // does not appear in `proxy_sliver.dart` at all. Upstream declares it
        // in exactly two places: `RenderOpacity` (`proxy_box.dart`,
        // `isRepaintBoundary => alwaysNeedsCompositing`) and
        // `RenderAnimatedOpacityMixin`, which is generic over `RenderObject`
        // and so covers the animated case on BOTH protocols. The STATIC sliver
        // opacity is the one node the mechanism cannot reach upstream — not
        // "the sliver protocol", which `RenderSliverAnimatedOpacity` is served
        // on through that mixin.
        //
        // FLUI has the path here because a retained capture is a flat list and
        // a node's own effect layers are addressable INSIDE the enclosing
        // boundary's capture, so nothing is promoted. Oracles (net-new — no
        // upstream test drives this setter, so nothing was replaced):
        // `a_sliver_alpha_change_updates_the_layer_without_repainting_the_subtree`
        // and `a_sliver_layer_update_is_written_back_into_the_retained_capture`
        // (`flui-rendering/tests/retained_boundary_layers.rs`).
        let mut impact = flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE;
        if old_needs_compositing != self.needs_compositing() {
            // Crossing the layered threshold changes which layers exist, not
            // just their properties. `COMPOSITING_BITS` implies `PAINT`, and
            // `apply_render_update_impact` marks paint before the layer
            // update, so the weaker mark refuses itself and the frame repaints
            // — which is the only way to serve a structural change.
            impact |= flui_rendering::RenderUpdateImpact::COMPOSITING_BITS;
        }
        if old_is_visible != (self.alpha > 0) {
            impact |= flui_rendering::RenderUpdateImpact::SEMANTICS;
        }
        // Starting or stopping suppressing the subtree's paint is a change to
        // what the frame CONTAINS, not to a layer property, and it is invisible
        // to the layer-update path: WITHOUT the always-compositing flag this
        // node emits no `OpacityLayer` at either alpha 255 or alpha 0, so it
        // has no effect-layer slot for an update to patch, and only a repaint
        // can add or remove that content.
        //
        // The flag is exactly the case this clause must NOT fire for, and it
        // does not: `skip_paint` is `alpha == 0 && !always_needs_compositing`,
        // so with the flag set it is false on both sides of the crossing and
        // nothing is added here — while `paint_alpha` keeps returning
        // `Some(0)`, so the layer survives and the patch has a slot after all.
        // That is the acceleration
        // `an_always_compositing_sliver_updates_its_layer_even_when_going_invisible`
        // pins.
        //
        // This is the honest impact rather than the last line of defence. The
        // paint phase refuses to graft when a node that requested an update
        // owns no slot in the capture, so it catches this case too — see
        // `RenderOpacity::set_opacity` (the box counterpart this mirrors) for
        // the mutation evidence that line earns its place.
        if old_skips_paint != <Self as RenderSliver>::skip_paint(self) {
            impact |= flui_rendering::RenderUpdateImpact::PAINT;
        }
        impact
    }

    /// Updates the explicit compositing predicate and reports its phase.
    pub fn set_always_needs_compositing(
        &mut self,
        value: bool,
    ) -> flui_rendering::RenderUpdateImpact {
        if self.always_needs_compositing == value {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.always_needs_compositing = value;
        flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
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

    // Compositing-layer requirement: the pipeline's compositing-bits walk
    // (`PipelineOwner::update_subtree_compositing_bits`, in
    // `pipeline/owner/compositing.rs`) reads `always_needs_compositing`
    // through `dyn RenderObject<SliverProtocol>`. Without this override the
    // blanket impl returns the default `false`, silently skipping the
    // dedicated compositing layer that the opacity effect requires.
    //
    // Upstream's `RenderSliverOpacity.alwaysNeedsCompositing` is
    // `child != null && _alpha > 0`. Two differences, both deliberate:
    // the child-presence gate is absorbed into the paint phase here, and the
    // alpha threshold diverges at 255 — see [`needs_compositing`]'s doc for
    // that one, which is a recorded divergence rather than parity. The
    // `always_needs_compositing` opt-in field is a FLUI extension
    // (stable-layer animation support) with no upstream counterpart; it is
    // OR-ed in, so it only ever widens when a layer is demanded.
    fn always_needs_compositing(&self) -> bool {
        self.needs_compositing()
    }

    // The whole point of RenderSliverOpacity: the pipeline reads paint_alpha
    // through `&dyn RenderObject<SliverProtocol>`; the blanket impl forwards here.
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
        // The DIVERGENT value, and the reason this assertion is worth its
        // line: upstream answers `true` here (`child != null && _alpha > 0`)
        // and has a test saying so. FLUI answers `false` because alpha 255
        // allocates no layer — see `needs_compositing`'s doc.
        assert!(!opaque.needs_compositing());

        let transparent = RenderSliverOpacity::transparent();
        assert_eq!(transparent.alpha(), 0);
        // This half IS parity: upstream's `_alpha > 0` is false at 0 too, and
        // the subtree is skipped entirely.
        assert!(!transparent.needs_compositing());
    }

    #[test]
    fn default_is_opaque() {
        let o = RenderSliverOpacity::default();
        assert!((o.opacity() - 1.0).abs() < f32::EPSILON);
        assert_eq!(o.alpha(), 255);
    }

    #[test]
    fn set_opacity_reports_exact_paint_compositing_and_visibility_impacts() {
        let mut o = RenderSliverOpacity::new(1.0);
        assert_eq!(o.set_opacity(1.0), flui_rendering::RenderUpdateImpact::NONE);
        assert_eq!(
            o.set_opacity(0.25),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
                | flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE,
            "entering the composited range is structural (COMPOSITING_BITS, which \
             implies PAINT) and the layer-update bit rides along; the owner applies \
             paint first, so the weaker mark refuses itself",
        );
        // 0.25 * 255 = 63.75 → round → 64.
        assert_eq!(o.alpha(), 64);
        assert_eq!(
            o.set_opacity(0.5),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE,
            "a visible change within the composited range lands only on the \
             OpacityLayer, so it updates that layer instead of repainting. \
             This is where FLUI improves on the reference rather than matching \
             it: upstream repaints here, because its update-only mechanism \
             needs the node to be a repaint boundary and the sliver opacity \
             is not one — see the setter's own comment",
        );
        assert_eq!(
            o.set_opacity(0.5),
            flui_rendering::RenderUpdateImpact::NONE,
            "an identical non-zero opacity is a no-op",
        );
        assert_eq!(
            o.set_opacity(1.0),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
                | flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE,
            "leaving the composited range is structural: COMPOSITING_BITS implies \
             PAINT and wins over the layer-update bit riding with it",
        );
        assert_eq!(
            o.set_opacity(0.0),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS
                | flui_rendering::RenderUpdateImpact::PAINT,
            "becoming invisible must REPAINT, not just update a layer: at both \
             alpha 255 and alpha 0 this node emits no OpacityLayer, so there is \
             no effect-layer slot for an update to patch and a graft would \
             replay the old, visible content",
        );
        assert_eq!(o.set_opacity(0.0), flui_rendering::RenderUpdateImpact::NONE,);
        assert_eq!(
            o.set_opacity(0.5),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
                | flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
            "becoming visible and composited affects both independent phases, and \
             the layer-update bit rides along with every value change",
        );
        assert_eq!(
            o.set_opacity(0.0),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
                | flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
            "composited straight to invisible: leaving the composited range and \
             starting to skip paint are both structural, so this repaints — the \
             transition the earlier sequence used to end on, kept because it is \
             the one an opacity animation actually finishes with",
        );
    }

    /// With `always_needs_compositing` set, alpha 0 keeps its `OpacityLayer`,
    /// so becoming invisible IS a pure layer property change.
    ///
    /// This is the one configuration where the new base impact changes an
    /// alpha-0 crossing: `paint_alpha` still returns `Some(0)` and
    /// `skip_paint` stays false, so a slot exists for the patch to land in and
    /// nothing structural moved. Without the flag the same transition must
    /// repaint (asserted above), which is what makes this a discriminating
    /// case rather than a restatement.
    #[test]
    fn an_always_compositing_sliver_updates_its_layer_even_when_going_invisible() {
        let mut o = RenderSliverOpacity::new(0.5);
        assert_eq!(
            o.set_always_needs_compositing(true),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS,
        );
        assert_eq!(o.paint_alpha(), Some(128));

        assert_eq!(
            o.set_opacity(0.0),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
            "the flag holds needs_compositing true and skip_paint false across \
             the crossing, so neither structural bit fires and the alpha change \
             is served by patching the layer that still exists",
        );
        assert_eq!(
            o.paint_alpha(),
            Some(0),
            "precondition for the impact above: the layer really does survive \
             at alpha 0, so there is a slot for the patch to address",
        );
        assert!(!o.skip_paint(), "and the subtree is still painted");
    }

    #[test]
    fn set_always_needs_compositing_returns_change_flag() {
        let mut o = RenderSliverOpacity::opaque();
        assert_eq!(
            o.set_always_needs_compositing(false),
            flui_rendering::RenderUpdateImpact::NONE
        );
        assert_eq!(
            o.set_always_needs_compositing(true),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
        );
        assert!(o.always_needs_compositing_flag());
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
        assert_eq!(
            o.set_always_needs_compositing(true),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS,
        );
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
        assert_eq!(
            o.set_always_needs_compositing(true),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS,
        );
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
