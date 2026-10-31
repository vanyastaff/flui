//! RenderOpacity - applies transparency to a single child.

use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Size};

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
    opacity: f64,
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
    pub fn new(opacity: f64) -> Self {
        let clamped = PaintOpacity::new(opacity).alpha();
        Self {
            opacity: clamped,
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
    pub fn opacity(&self) -> f64 {
        self.opacity
    }

    /// Sets the opacity value.
    ///
    /// # Arguments
    ///
    /// * `opacity` - Opacity value (0.0 = transparent, 1.0 = opaque).
    pub fn set_opacity(&mut self, opacity: f64) -> flui_rendering::RenderUpdateImpact {
        let clamped = PaintOpacity::new(opacity).alpha();
        if self.opacity == clamped {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        let old_needs_compositing = self.needs_compositing();
        let old_is_visible = self.opacity > 0.0;
        // Whether this node suppresses its subtree's paint entirely. Read
        // through the trait method rather than re-deriving `alpha == 0`, so
        // this stays correct if that predicate changes.
        let old_skips_paint = <Self as RenderBox>::skip_paint(self);
        self.opacity = clamped;
        // A pure alpha change lands ONLY on the `OpacityLayer` this node
        // pushes, so the frame can rebuild that layer and replay the enclosing
        // repaint boundary's retained output rather than repainting the
        // subtree, hence a composited-layer update rather than a paint mark.
        let mut impact = flui_rendering::RenderUpdateImpact::COMPOSITED_LAYER_UPDATE;
        if old_needs_compositing != self.needs_compositing() {
            // Crossing the layered threshold changes which layers exist, not
            // just their properties. `COMPOSITING_BITS` implies `PAINT`, and
            // `apply_render_update_impact` marks paint before the layer
            // update, so the weaker mark refuses itself and the frame repaints
            // — which is the only way to serve a structural change.
            impact |= flui_rendering::RenderUpdateImpact::COMPOSITING_BITS;
        }
        if old_is_visible != (self.opacity > 0.0) {
            impact |= flui_rendering::RenderUpdateImpact::SEMANTICS;
        }
        // Starting or stopping suppressing the subtree's paint is a change to
        // what the frame CONTAINS, not to a layer property, and it is invisible
        // to the layer-update path: WITHOUT the always-compositing flag this
        // node emits no `OpacityLayer` at either alpha 1 or alpha 0, so it
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
    /// is non-trivially blended (`0 < alpha < 1`). Fully-transparent
    /// (`alpha == 0`) does not need compositing because the subtree is skipped
    /// entirely.
    pub fn needs_compositing(&self) -> bool {
        self.always_needs_compositing || (self.opacity > 0.0 && self.opacity < 1.0)
    }
}

impl Default for RenderOpacity {
    fn default() -> Self {
        Self::opaque()
    }
}

impl flui_foundation::inspect::Inspect for RenderOpacity {
    fn kind(&self) -> &'static str {
        "RenderOpacity"
    }

    fn inspect(&self, out: &mut flui_foundation::inspect::Fields<'_>) {
        out.field("opacity", &self.opacity)
            .unit(flui_foundation::inspect::Unit::Fraction)
            .default(&1.0_f64);
        out.field("always_needs_compositing", &self.always_needs_compositing)
            .default(&false);
    }
}
impl RenderBox for RenderOpacity {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = *ctx.constraints();

        if ctx.child_count() > 0 {
            self.has_child = true;

            // Layout child with same constraints
            ctx.layout_child(0, constraints)
        } else {
            self.has_child = false;
            // No child - take minimum size
            Ok(constraints.smallest())
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    fn visits_child_for_semantics(&self, _child_slot: usize, _size: Size) -> bool {
        self.opacity > 0.0
    }

    fn always_needs_compositing(&self) -> bool {
        self.needs_compositing()
    }

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
        // None when fully opaque (1) OR fully transparent (0) without the
        // always-needs-compositing flag: neither requires an OpacityLayer.
        if (self.opacity == 1.0 || self.opacity == 0.0) && !self.always_needs_compositing {
            PaintEffects::NONE
        } else {
            PaintEffects::NONE.with_opacity(PaintOpacity::new(self.opacity))
        }
    }

    fn skip_paint(&self) -> bool {
        // Fully transparent without the always-compositing flag: suppress child
        // paint entirely.
        self.opacity == 0.0 && !self.always_needs_compositing
    }
}
