//! RenderOpacity - applies transparency to a single child.

use flui_tree::Single;
use flui_types::{Offset, Size};

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    traits::{PaintEffects, PaintOpacity, RenderBox},
};

/// A render object that applies transparency to its child.
///
/// The opacity value ranges from 0.0 (fully transparent) to 1.0 (fully opaque).
/// When opacity is 0.0, the child is completely invisible but still takes up
/// space and can receive hit tests.
///
/// # Performance
///
/// Opacity creates a compositing layer, which has some performance cost.
/// For animating opacity, consider using `RenderAnimatedOpacity` which can
/// optimize the case where opacity changes frequently.
///
/// # Example
///
/// ```ignore
/// let opacity = RenderOpacity::new(0.5); // 50% transparent
/// // Use with PipelineOwner and RenderTree for actual rendering
/// // Add a child, then layout with constraints
/// ```
#[derive(Debug, Clone)]
pub struct RenderOpacity {
    /// Opacity value (0.0 = transparent, 1.0 = opaque).
    opacity: f32,
    /// Alpha as u8 (0-255) for efficient layer operations.
    alpha: u8,
    /// Whether we have a child.
    has_child: bool,
    /// Whether opacity is always needed for compositing.
    /// When true, we always create an opacity layer.
    /// When false, we skip the layer when opacity is 1.0.
    always_needs_compositing: bool,
}

impl RenderOpacity {
    /// Creates a new opacity render object with the given opacity.
    ///
    /// # Arguments
    ///
    /// * `opacity` - Opacity value (0.0 = transparent, 1.0 = opaque). Values
    ///   outside [0.0, 1.0] are clamped.
    pub fn new(opacity: f32) -> Self {
        let clamped = opacity.clamp(0.0, 1.0);
        Self {
            opacity: clamped,
            alpha: Self::opacity_to_alpha(clamped),
            has_child: false,
            always_needs_compositing: false,
        }
    }

    /// Creates a fully opaque render object (opacity = 1.0).
    pub fn opaque() -> Self {
        Self::new(1.0)
    }

    /// Creates a fully transparent render object (opacity = 0.0).
    pub fn transparent() -> Self {
        Self::new(0.0)
    }

    /// Returns the current opacity value.
    pub fn opacity(&self) -> f32 {
        self.opacity
    }

    /// Sets the opacity value.
    ///
    /// # Arguments
    ///
    /// * `opacity` - Opacity value (0.0 = transparent, 1.0 = opaque).
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
        let old_skips_paint = <Self as RenderBox>::skip_paint(self);
        self.opacity = clamped;
        self.alpha = Self::opacity_to_alpha(clamped);
        // A pure alpha change lands ONLY on the `OpacityLayer` this node
        // pushes, so the frame can rebuild that layer and replay the enclosing
        // repaint boundary's retained output rather than repainting the
        // subtree. Flutter's own setter does exactly this
        // (`proxy_box.dart`, `RenderOpacity.opacity`: `markNeedsCompositedLayerUpdate()`,
        // not `markNeedsPaint()`).
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
        // nothing is added here — while `paint_effects` keeps returning an
        // opacity effect at alpha 0, so the layer survives and the patch has
        // a slot after all.
        //
        // This is the honest impact rather than the last line of defence. The
        // paint phase refuses to graft when a node that requested an update
        // owns no slot in the capture, so it catches this case too — verified
        // by mutation: removing the line below leaves every behavioural test
        // green and trips only the impact assertion in this file. Reporting it
        // here still earns its place, because a caller that reports the truth
        // saves the frame a queue-then-refuse round trip, and because the two
        // guards protect a class that has already produced visible corruption
        // once.
        if old_skips_paint != <Self as RenderBox>::skip_paint(self) {
            impact |= flui_rendering::RenderUpdateImpact::PAINT;
        }
        impact
    }

    /// Returns the alpha value (0-255).
    pub fn alpha(&self) -> u8 {
        self.alpha
    }

    /// Sets whether this render object always needs compositing.
    ///
    /// When true, an opacity layer is always created even when opacity is 1.0.
    /// This can be useful for animations where you want consistent compositing
    /// behavior.
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

    /// Returns whether compositing is needed.
    ///
    /// Returns `true` only when `always_needs_compositing` is set OR the alpha
    /// is non-trivially blended (`0 < alpha < 255`). Fully-transparent
    /// (`alpha == 0`) does not need compositing because the subtree is skipped
    /// entirely — Flutter parity: `alwaysNeedsCompositing => alpha > 0`.
    pub fn needs_compositing(&self) -> bool {
        self.always_needs_compositing || (self.alpha > 0 && self.alpha != 255)
    }

    /// Converts opacity (0.0-1.0) to alpha (0-255).
    fn opacity_to_alpha(opacity: f32) -> u8 {
        (opacity * 255.0).round() as u8
    }
}

impl Default for RenderOpacity {
    fn default() -> Self {
        Self::opaque()
    }
}

impl flui_foundation::Diagnosticable for RenderOpacity {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_default_double("opacity", self.opacity, 1.0, None);
        properties.add_flag(
            "always_needs_compositing",
            self.always_needs_compositing,
            "always needs compositing",
        );
    }
}
impl RenderBox for RenderOpacity {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();

        if ctx.child_count() > 0 {
            self.has_child = true;

            // Layout child with same constraints
            ctx.layout_child(0, constraints)
        } else {
            self.has_child = false;
            // No child - take minimum size
            constraints.smallest()
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    // paint() uses default no-op - opacity is applied via paint_effects()

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // Invisible elements can still receive hit tests
        if !ctx.is_within_own_size() {
            return false;
        }

        if self.has_child {
            ctx.hit_test_child_at_offset(0, Offset::ZERO)
        } else {
            false
        }
    }

    // The whole point of RenderOpacity: the pipeline reads paint_effects
    // through `&dyn RenderObject<BoxProtocol>`; the blanket impl forwards
    // here.
    fn paint_effects(&self, _size: Size) -> PaintEffects {
        // None when fully opaque (255) OR fully transparent (0) without the
        // always-needs-compositing flag: neither requires an OpacityLayer.
        // Flutter: alpha=0 → layer=null (no layer needed).
        if (self.alpha == 255 || self.alpha == 0) && !self.always_needs_compositing {
            PaintEffects::NONE
        } else {
            PaintEffects::NONE.with_opacity(PaintOpacity::new(self.alpha))
        }
    }

    fn skip_paint(&self) -> bool {
        // Flutter RenderOpacity.paint: `if (_alpha == 0) { return; }`
        // Fully transparent without the always-compositing flag: suppress child
        // paint entirely.
        self.alpha == 0 && !self.always_needs_compositing
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_opacity_new() {
        let opacity = RenderOpacity::new(0.5);
        assert!((opacity.opacity() - 0.5).abs() < f32::EPSILON);
        assert_eq!(opacity.alpha(), 128); // 0.5 * 255 ≈ 128
    }

    #[test]
    fn test_opacity_clamp() {
        let opacity = RenderOpacity::new(1.5);
        assert!((opacity.opacity() - 1.0).abs() < f32::EPSILON);
        assert_eq!(opacity.alpha(), 255);

        let opacity = RenderOpacity::new(-0.5);
        assert!((opacity.opacity() - 0.0).abs() < f32::EPSILON);
        assert_eq!(opacity.alpha(), 0);
    }

    #[test]
    fn test_opacity_opaque() {
        let opacity = RenderOpacity::opaque();
        assert!((opacity.opacity() - 1.0).abs() < f32::EPSILON);
        assert_eq!(opacity.alpha(), 255);
        assert!(!opacity.needs_compositing());
    }

    #[test]
    fn test_opacity_transparent() {
        let opacity = RenderOpacity::transparent();
        assert!((opacity.opacity() - 0.0).abs() < f32::EPSILON);
        assert_eq!(opacity.alpha(), 0);
        // Flutter: alwaysNeedsCompositing => alpha > 0, so alpha=0 must NOT
        // need compositing (the subtree is skipped entirely).
        assert!(!opacity.needs_compositing());
    }

    #[test]
    fn test_opacity_set() {
        let mut opacity = RenderOpacity::new(1.0);
        assert_eq!(
            opacity.set_opacity(0.25),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
                | flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE,
            "entering the composited range is structural (COMPOSITING_BITS, which \
             implies PAINT) and the layer-update bit rides along; the owner applies \
             paint first, so the weaker mark refuses itself",
        );
        assert!((opacity.opacity() - 0.25).abs() < f32::EPSILON);
        assert_eq!(opacity.alpha(), 64); // 0.25 * 255 ≈ 64
    }

    #[test]
    fn test_needs_compositing() {
        let mut opacity = RenderOpacity::new(1.0);
        assert!(!opacity.needs_compositing());

        assert_eq!(
            opacity.set_opacity(0.5),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
                | flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE,
        );
        assert!(opacity.needs_compositing());

        assert_eq!(
            opacity.set_opacity(1.0),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
                | flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE,
        );
        assert_eq!(
            opacity.set_always_needs_compositing(true),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS,
        );
        assert!(opacity.needs_compositing());
    }

    #[test]
    fn opacity_impacts_track_paint_compositing_and_semantics_independently() {
        let mut opacity = RenderOpacity::new(0.25);
        assert_eq!(
            opacity.set_opacity(0.5),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE,
            "a visible change within the composited range lands only on the \
             OpacityLayer, so it updates that layer instead of repainting — \
             Flutter's `RenderOpacity.opacity` calls markNeedsCompositedLayerUpdate",
        );
        assert_eq!(
            opacity.set_opacity(0.5),
            flui_rendering::RenderUpdateImpact::NONE,
            "an identical non-zero opacity is a no-op",
        );
        assert_eq!(
            opacity.set_opacity(1.0),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
                | flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE,
            "leaving the composited range is structural: COMPOSITING_BITS implies \
             PAINT and wins over the layer-update bit riding with it",
        );
        assert_eq!(
            opacity.set_opacity(0.0),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS
                | flui_rendering::RenderUpdateImpact::PAINT,
            "becoming invisible must REPAINT, not just update a layer: at both \
             alpha 255 and alpha 0 this node emits no OpacityLayer, so there is \
             no effect-layer slot for an update to patch and a graft would \
             replay the old, visible content",
        );
        assert_eq!(
            opacity.set_opacity(0.0),
            flui_rendering::RenderUpdateImpact::NONE,
        );
        assert_eq!(
            opacity.set_opacity(0.5),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS
                | flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
            "becoming visible and composited affects both independent phases, and \
             the layer-update bit rides along with every value change",
        );
    }

    #[test]
    fn test_default() {
        let opacity = RenderOpacity::default();
        assert!((opacity.opacity() - 1.0).abs() < f32::EPSILON);
    }

    // 1.3 RED→GREEN: alpha=0 must return None from paint_effects's opacity
    // (no layer), not Some(0). Flutter RenderOpacity.paint: alpha=0 →
    // layer=null. Before fix: returned Some(0). After fix: returns None.
    #[test]
    fn paint_effects_opacity_returns_none_when_transparent() {
        let o = RenderOpacity::transparent(); // alpha = 0
        assert_eq!(
            RenderBox::paint_effects(&o, Size::ZERO)
                .opacity
                .map(|o| o.alpha),
            None,
            "alpha=0 without always-flag must return None (no OpacityLayer); \
             Flutter: alpha=0 → layer=null"
        );
    }

    #[test]
    fn paint_effects_opacity_returns_none_when_opaque() {
        let o = RenderOpacity::opaque();
        assert_eq!(
            RenderBox::paint_effects(&o, Size::ZERO)
                .opacity
                .map(|o| o.alpha),
            None
        );
    }

    #[test]
    fn paint_effects_opacity_returns_some_for_partial() {
        let o = RenderOpacity::new(0.5);
        assert_eq!(
            RenderBox::paint_effects(&o, Size::ZERO)
                .opacity
                .map(|o| o.alpha),
            Some(128)
        );
    }

    #[test]
    fn skip_paint_true_when_transparent() {
        assert!(RenderOpacity::transparent().skip_paint());
        assert!(!RenderOpacity::opaque().skip_paint());
        assert!(!RenderOpacity::new(0.5).skip_paint());
    }

    // alpha=0 WITH always-flag: paint_effects returns an opacity(0),
    // skip_paint false.
    #[test]
    fn paint_effects_opacity_returns_some_when_transparent_but_forced() {
        let mut o = RenderOpacity::transparent();
        assert_eq!(
            o.set_always_needs_compositing(true),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS,
        );
        assert_eq!(
            RenderBox::paint_effects(&o, Size::ZERO)
                .opacity
                .map(|o| o.alpha),
            Some(0)
        );
        assert!(!o.skip_paint());
    }

    /// With `always_needs_compositing` set, alpha 0 keeps its `OpacityLayer`,
    /// so becoming invisible IS a pure layer property change.
    ///
    /// The one configuration where the `COMPOSITED_LAYER_UPDATE` base changes
    /// an alpha-0 crossing: `paint_effects` still returns an opacity effect
    /// at alpha 0 and `skip_paint` stays false, so a slot exists for the
    /// patch and nothing structural moved. Without the flag the same
    /// transition must repaint —
    /// asserted in `opacity_impacts_track_paint_compositing_and_semantics_independently`
    /// — which is what makes this a discriminating case rather than a
    /// restatement, and what the setter's `skip_paint` comment claims.
    #[test]
    fn an_always_compositing_opacity_updates_its_layer_even_when_going_invisible() {
        let mut o = RenderOpacity::new(0.5);
        assert_eq!(
            o.set_always_needs_compositing(true),
            flui_rendering::RenderUpdateImpact::COMPOSITING_BITS,
        );
        assert_eq!(
            RenderBox::paint_effects(&o, Size::ZERO)
                .opacity
                .map(|o| o.alpha),
            Some(128)
        );

        assert_eq!(
            o.set_opacity(0.0),
            flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE
                | flui_rendering::RenderUpdateImpact::SEMANTICS,
            "the flag holds needs_compositing true and skip_paint false across \
             the crossing, so neither structural bit fires and the alpha change \
             is served by patching the layer that still exists",
        );
        assert_eq!(
            RenderBox::paint_effects(&o, Size::ZERO)
                .opacity
                .map(|o| o.alpha),
            Some(0),
            "precondition for the impact above: the layer really does survive \
             at alpha 0, so there is a slot for the patch to address",
        );
        assert!(!o.skip_paint(), "and the subtree is still painted");
    }
}
