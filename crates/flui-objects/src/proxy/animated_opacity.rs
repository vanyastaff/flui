//! `RenderAnimatedOpacity` — applies a continuously-animated transparency to
//! a single child, driven by an injected, hot-swappable
//! [`ProxyAnimation<f64>`].
//!
//! # Dirty-marking rule
//!
//! The object caches an alpha, recomputes it on every `opacity` animation tick
//! via a listener registered in `attach` (and once more, unconditionally,
//! right after registering — in case it changed while nothing was
//! listening). A partial-alpha change updates the retained layer; crossing
//! the layered threshold also updates compositing bits, and crossing zero
//! requests paint and semantics.
//!
//! # Composited-layer updates
//!
//! A partial-alpha tick reuses the child subtree and updates the cached layer.
//! Crossing zero adds or removes the child from painting and, unless explicitly
//! retained, from semantics. Layer updates do not promote the node to a repaint
//! boundary: the frame rebuilds just this node's own effect layers inside the
//! ENCLOSING repaint boundary's retained capture, so the node stays an ordinary
//! non-boundary. The design and its accepted trade-offs are recorded in
//! `flui-rendering/ARCHITECTURE.md`, "A composited-layer update patches the
//! enclosing capture; no node is promoted to a boundary".
//!
//! **Which seam a tick takes matters, and it is not the setter one.** This type
//! has no impact-returning setter at all: an animation tick runs through the
//! listener, and `recompute_alpha` pokes a [`LocalRenderInvalidationHandle`]
//! directly. A plain alpha move updates the composited layer; crossing the
//! layered threshold also updates compositing bits; crossing alpha zero
//! requests paint and semantics. The sample is published before all required phases are
//! admitted together through `request_update`. The owner retains those marks
//! even if its wake callback fails, without requiring another animation tick.
//!
//! # Retargeting — the proxy absorbs the swap
//!
//! A configuration change that swaps to a new controller/curve would
//! normally require re-subscribing the tick listener to the new
//! animation. This render object never sees that swap: it is handed a
//! [`ProxyAnimation<f64>`] once, at construction, and listens to that SAME
//! proxy for its entire lifetime. The `AnimatedOpacity` widget's state (`
//! flui-widgets/src/animated/animated_opacity.rs`) owns the proxy and, on
//! retarget, swaps its *parent* animation (`ProxyAnimation::set_parent`) —
//! the proxy re-fires this render object's listener with the new parent's
//! curve/tween composition, and no field or subscription here ever changes.
//! Re-subscription is unnecessary not because no retarget path exists, but
//! because the proxy absorbs it entirely on the widget side; this render
//! object never bakes a curve of its own.

use std::rc::Rc;

use crate::opacity_sample::OpacitySample;
use crate::subscription::RenderSubscription;

use flui_foundation::Single;
use flui_foundation::geometry::{Offset, Size};

use flui_animation::{Animation, ProxyAnimation};
use flui_foundation::panic::PanicRecovery;

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    pipeline::LocalRenderInvalidationHandle,
    traits::{PaintEffects, PaintOpacity, RenderBox},
};

/// A render object that applies a continuously-animated transparency to its
/// child.
///
/// Unlike [`RenderOpacity`](crate::RenderOpacity), the alpha is not set
/// directly — it tracks an injected, composed [`ProxyAnimation<f64>`] (curve
/// and retarget algebra owned by the calling widget) and updates itself on
/// every animation tick via a listener registered in
/// [`attach`](RenderBox::attach). See the module docs for the exact
/// dirty-marking rule, and
/// the *Retargeting* section for how a configuration change reaches this
/// object without ever replacing it.
///
/// # Performance
///
/// Alpha changes update the retained effect layer. Crossing alpha zero
/// requests paint because the child enters or leaves the painted subtree.
pub struct RenderAnimatedOpacity {
    /// The composed animation driving alpha, injected at construction and
    /// listened to for this object's entire lifetime. A
    /// [`ProxyAnimation<f64>`] so the owning widget can hot-swap the
    /// *parent* it wraps (retarget) without this render object ever seeing a
    /// new listenable — see the module docs' *Retargeting* section.
    animation: ProxyAnimation<f64>,
    /// Owner-local alpha shared with the tick listener and paint walk.
    /// Sampling generations prevent an older reentrant read from replacing
    /// a newer sample; no borrow survives the animation's value callback.
    alpha: Rc<OpacitySample>,
    /// Whether child semantics remain included at zero opacity.
    always_include_semantics: bool,
    /// Tick subscription, cancelled on detach or owner release.
    listener_id: Option<RenderSubscription>,
}

impl RenderAnimatedOpacity {
    /// Creates a render object driven by `animation`, an already-composed
    /// [`ProxyAnimation<f64>`] (curve and controller ownership live entirely
    /// on the widget side — this object never constructs either, matching
    /// [`RenderAnimatedSize::new`](crate::RenderAnimatedSize::new)'s
    /// already-built-controller shape one level further: it does not even
    /// see the controller, only the composed value stream).
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

    /// Publishes a changed alpha and admits its required phases together.
    /// Returns whether this sample changed alpha and reached a live owner.
    ///
    /// Free function (not `&self`) so both [`attach`](RenderBox::attach)
    /// (called with the live `self.animation`/`self.alpha`) and the tick
    /// listener closure (called with cloned copies, since the closure must
    /// be `'static` and cannot borrow `self`) share one implementation.
    fn recompute_alpha(
        animation: &ProxyAnimation<f64>,
        alpha: &OpacitySample,
        handle: &LocalRenderInvalidationHandle,
    ) -> bool {
        alpha.sample(|| animation.value(), handle)
    }
}

impl std::fmt::Debug for RenderAnimatedOpacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderAnimatedOpacity")
            .field("opacity", &self.opacity())
            .field("always_include_semantics", &self.always_include_semantics)
            .finish_non_exhaustive()
    }
}

impl flui_foundation::Diagnosticable for RenderAnimatedOpacity {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_default_double("opacity", self.opacity(), 1.0, None);
        properties.add_flag(
            "always_include_semantics",
            self.always_include_semantics,
            "always include semantics",
        );
    }
}

impl RenderBox for RenderAnimatedOpacity {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            ctx.layout_child(0, constraints)
        } else {
            Ok(constraints.smallest())
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // An animated-opacity node hit-tests regardless of alpha, same as
        // `RenderOpacity` (see `proxy/opacity.rs`).
        if !ctx.is_within_own_size() {
            return false;
        }
        ctx.hit_test_child_at_offset(0, Offset::ZERO)
    }

    // The whole point of this object: the pipeline reads paint_effects
    // through `&dyn RenderObject<BoxProtocol>`; the blanket impl forwards
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
        // Fully transparent: nothing to paint.
        self.opacity() == 0.0
    }

    fn visits_child_for_semantics(&self, _child_slot: usize, _size: Size) -> bool {
        self.always_include_semantics || self.opacity() > 0.0
    }

    // Compositing-layer requirement: the pipeline's compositing-bits walk
    // (`PipelineOwner::update_subtree_compositing_bits`) reads
    // `always_needs_compositing` through `dyn RenderObject<BoxProtocol>`,
    // exactly as it does for slivers (see
    // `RenderSliverAnimatedOpacity::always_needs_compositing`'s comment for
    // the walk's mechanics). Without this override, `recompute_alpha`'s
    // `mark_needs_compositing_bits_update()` calls would be inert: the
    // dirty bit fires a re-walk, but the walk would still read the
    // `RenderBox` trait's default `false` (`render_box.rs`'s
    // `always_needs_compositing` default) and never allocate the layer.
    //
    // The threshold is `OpacitySample::is_layered` (`0 < alpha < 1`),
    // the SAME predicate `paint_effects`/`skip_paint` above already use, and
    // the one `RenderOpacity`/`RenderSliverOpacity` establish for this crate:
    // at `alpha == 1` no layer is ever allocated (`paint_effects` returns no
    // opacity effect), so requiring compositing there would be pure
    // overhead with no visual effect: fully opaque does not count as
    // needing its own layer.
    //
    // The static box and sliver variants expose the same requirement through
    // their protocol hooks, including their explicit force-layer setting.
    fn always_needs_compositing(&self) -> bool {
        OpacitySample::is_layered(self.opacity())
    }

    fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        self.detach();
        let mut recovery = PanicRecovery::new();
        let animation = self.animation.clone();
        let alpha = self.alpha.clone();
        let mark_handle = handle.clone();
        // Registered on the proxy itself, not on whatever it currently wraps
        // — see the module docs' *Retargeting* section: a widget-side
        // `set_parent` swap re-fires this same subscription with the new
        // parent's values, so it never needs to move.
        self.listener_id = RenderSubscription::subscribe_callback(
            || Some(Rc::new(self.animation.clone())),
            Rc::new(move || {
                Self::recompute_alpha(&animation, &alpha, &mark_handle);
            }),
            &mut recovery,
        );
        recovery.finish();

        // Refresh once more in case the animation's value changed while this
        // node wasn't listening (e.g. a detach/reattach).
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
