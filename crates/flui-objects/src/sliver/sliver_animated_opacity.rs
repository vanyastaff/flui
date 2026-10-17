//! `RenderSliverAnimatedOpacity` — applies a continuously-animated
//! transparency to a single sliver child, driven by an injected,
//! hot-swappable [`ProxyAnimation<f64>`].
//!
//! # Relation to the box variant
//!
//! It uses the SAME alpha-caching/dirty-marking rule the box variant uses.
//! See [`RenderAnimatedOpacity`](crate::RenderAnimatedOpacity)'s module docs
//! for the full mechanism (alpha caching, the
//! paint/compositing-bits marking rule, the `is_layered` predicate both
//! variants use for `always_needs_compositing`, and the *Retargeting*
//! section explaining why no re-subscription is needed here — the
//! proxy absorbs it on the widget side) — this module only restates the
//! sliver-specific contract surface (`RenderSliverOpacity`'s layout/hit-test
//! passthrough).
//!
//! `flui-widgets`' `SliverFadeTransition` owns a stable proxy and retargets its
//! parent when the supplied animation changes. The render subscription ends on
//! detach or owner release; animation-run lifetime remains with the caller.

use std::rc::Rc;

use crate::opacity_sample::OpacitySample;
use crate::subscription::RenderSubscription;

use flui_foundation::Single;
use flui_foundation::geometry::Size;

use flui_animation::{Animation, ProxyAnimation};
use flui_foundation::panic::PanicRecovery;

use flui_rendering::{
    constraints::SliverGeometry,
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    pipeline::LocalRenderInvalidationHandle,
    traits::{PaintEffects, PaintOpacity, RenderSliver},
};

/// A sliver render object that applies a continuously-animated transparency
/// to its single sliver child.
///
/// Mirrors [`RenderAnimatedOpacity`](crate::RenderAnimatedOpacity) over
/// [`RenderSliverOpacity`](crate::RenderSliverOpacity)'s contract instead of
/// `RenderOpacity`'s. See the module docs.
pub struct RenderSliverAnimatedOpacity {
    /// The composed animation driving alpha. Constructor-injected only —
    /// see [`RenderAnimatedOpacity`](crate::RenderAnimatedOpacity)'s field
    /// doc for why no post-construction swap setter exists (a
    /// [`ProxyAnimation<f64>`] absorbs retargeting on the widget side
    /// instead).
    animation: ProxyAnimation<f64>,
    /// Owner-local alpha shared with the tick listener and paint walk.
    /// Both opacity protocols use the same generation-checked sample and
    /// admit every required invalidation phase before waking the owner.
    alpha: Rc<OpacitySample>,
    /// Whether child semantics are included regardless of alpha.
    always_include_semantics: bool,
    /// Tick subscription, cancelled on detach or owner release.
    listener_id: Option<RenderSubscription>,
}

impl RenderSliverAnimatedOpacity {
    /// Creates a render object driven by `animation`, an already-composed
    /// [`ProxyAnimation<f64>`] (never constructs one itself — see
    /// [`RenderAnimatedOpacity::new`](crate::RenderAnimatedOpacity::new)).
    #[must_use]
    pub fn new(animation: ProxyAnimation<f64>, always_include_semantics: bool) -> Self {
        let alpha = Rc::new(OpacitySample::new(animation.value()));
        Self {
            animation,
            alpha,
            always_include_semantics,
            listener_id: None,
        }
    }

    /// Returns the accepted opacity in `0.0..=1.0`, refreshed on animation
    /// notifications and construction or attachment.
    #[inline]
    #[must_use]
    pub fn opacity(&self) -> f64 {
        self.alpha.get()
    }

    /// Whether child semantics are included regardless of alpha.
    #[inline]
    #[must_use]
    pub fn always_include_semantics(&self) -> bool {
        self.always_include_semantics
    }

    /// Recomputes `alpha` and marks the node dirty through `handle`. See
    /// [`RenderAnimatedOpacity`](crate::RenderAnimatedOpacity)'s
    /// `recompute_alpha` for the shared sample publication and phase admission
    /// rule. Returns whether alpha changed and reached a live owner.
    fn recompute_alpha(
        animation: &ProxyAnimation<f64>,
        alpha: &OpacitySample,
        handle: &LocalRenderInvalidationHandle,
    ) -> bool {
        alpha.sample(|| animation.value(), handle)
    }
}

impl std::fmt::Debug for RenderSliverAnimatedOpacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderSliverAnimatedOpacity")
            .field("opacity", &self.opacity())
            .field("always_include_semantics", &self.always_include_semantics)
            .finish_non_exhaustive()
    }
}

impl flui_foundation::Diagnosticable for RenderSliverAnimatedOpacity {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_default_double("opacity", self.opacity(), 1.0, None);
        builder.add_flag(
            "always_include_semantics",
            self.always_include_semantics,
            "always include semantics",
        );
    }
}

impl RenderSliver for RenderSliverAnimatedOpacity {
    type Arity = Single;
    type ParentData = SliverPhysicalParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut SliverLayoutContext<'_, Single, SliverPhysicalParentData>,
    ) -> flui_rendering::RenderResult<SliverGeometry> {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            // Transparent passthrough — opacity does not affect layout.
            ctx.layout_child(0, constraints)
        } else {
            Ok(SliverGeometry::ZERO)
        }
    }

    fn hit_test(
        &self,
        ctx: &mut SliverHitTestContext<'_, Single, SliverPhysicalParentData>,
    ) -> bool {
        // Same as `RenderSliverOpacity` — hit-tests regardless of alpha.
        ctx.hit_test_child_at_layout_offset(0)
    }

    // Mirrors `RenderSliverOpacity::always_needs_compositing`: the sliver
    // compositing-bits walk reads this through
    // `dyn RenderObject<SliverProtocol>` (see `sliver/sliver_opacity.rs`'s
    // comment on that override for the walk's mechanics). The box variant,
    // `RenderAnimatedOpacity`, carries the analogous override for the same
    // reason — see its own comment for the predicate used
    // (`is_layered`, `0 < alpha < 1`) and why.
    fn always_needs_compositing(&self) -> bool {
        OpacitySample::is_layered(self.opacity())
    }

    // The whole point of this object: the pipeline reads paint_effects
    // through `&dyn RenderObject<SliverProtocol>`; the blanket impl forwards
    // here.
    fn paint_effects(&self, _size: Size) -> PaintEffects {
        let alpha = self.opacity();
        // None when fully opaque (1) or fully transparent (0): neither
        // requires an OpacityLayer.
        if alpha == 1.0 || alpha == 0.0 {
            PaintEffects::NONE
        } else {
            PaintEffects::NONE.with_opacity(PaintOpacity::new(alpha))
        }
    }

    fn skip_paint(&self) -> bool {
        self.opacity() == 0.0
    }

    fn visits_child_for_semantics(&self, _child_slot: usize, _size: Size) -> bool {
        self.always_include_semantics || self.opacity() > 0.0
    }

    fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        self.detach();
        let mut recovery = PanicRecovery::new();
        let animation = self.animation.clone();
        let alpha = self.alpha.clone();
        let mark_handle = handle.clone();
        self.listener_id = RenderSubscription::subscribe_callback(
            || Some(Rc::new(self.animation.clone())),
            Rc::new(move || {
                Self::recompute_alpha(&animation, &alpha, &mark_handle);
            }),
            &mut recovery,
        );
        recovery.finish();
        Self::recompute_alpha(&self.animation, &self.alpha, &handle);
    }

    fn detach(&mut self) {
        if let Some(subscription) = self.listener_id.take() {
            let mut recovery = PanicRecovery::new();
            subscription.close(&mut recovery);
            recovery.finish();
        }
    }
}
