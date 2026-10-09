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
//! # Zero-consumer honesty
//!
//! No widget in `flui-widgets` constructs this render object yet; wiring a
//! sliver-flavored `AnimatedOpacity` to it is a deferred follow-up, same as
//! before this change — only the box variant's constructor signature and its
//! widget were rewired in this unit.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use flui_foundation::Single;
use flui_foundation::geometry::Size;

use flui_animation::{Animation, ProxyAnimation};
use flui_foundation::{Listenable, ListenerId};

use flui_rendering::{
    constraints::SliverGeometry,
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    pipeline::RenderInvalidationHandle,
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
    /// Alpha cache (`0..=255`), shared with the tick listener closure.
    /// See [`RenderAnimatedOpacity`](crate::RenderAnimatedOpacity)'s
    /// field doc for why this is an `AtomicU8`, not a `Cell`/`Mutex`, and
    /// why `Ordering::Relaxed` suffices (the dirty-channel send in
    /// [`recompute_alpha`](Self::recompute_alpha), not the atomic op, is
    /// the synchronization edge).
    alpha: Arc<AtomicU8>,
    /// Whether child semantics are included regardless of alpha.
    always_include_semantics: bool,
    /// Tick-listener subscription on `animation`, torn down in `detach`.
    listener_id: Option<ListenerId>,
}

impl RenderSliverAnimatedOpacity {
    /// Creates a render object driven by `animation`, an already-composed
    /// [`ProxyAnimation<f64>`] (never constructs one itself — see
    /// [`RenderAnimatedOpacity::new`](crate::RenderAnimatedOpacity::new)).
    #[must_use]
    pub fn new(animation: ProxyAnimation<f64>, always_include_semantics: bool) -> Self {
        let alpha = Arc::new(AtomicU8::new(Self::opacity_to_alpha(animation.value())));
        Self {
            animation,
            alpha,
            always_include_semantics,
            listener_id: None,
        }
    }

    /// Returns the current cached alpha (`0..=255`).
    #[inline]
    #[must_use]
    pub fn alpha(&self) -> u8 {
        self.alpha.load(Ordering::Relaxed)
    }

    /// The composed animation's raw `f64` value, bypassing the `u8` alpha
    /// cache's `1/255` quantization. See
    /// [`RenderAnimatedOpacity::opacity_value`](crate::RenderAnimatedOpacity::opacity_value)
    /// for why this exists.
    #[inline]
    #[must_use]
    pub fn opacity_value(&self) -> f64 {
        self.animation.value()
    }

    /// Whether child semantics are included regardless of alpha.
    #[inline]
    #[must_use]
    pub fn always_include_semantics(&self) -> bool {
        self.always_include_semantics
    }

    /// Converts opacity (`0.0..=1.0`) to alpha (`0..=255`).
    #[inline]
    fn opacity_to_alpha(opacity: f64) -> u8 {
        (opacity.clamp(0.0, 1.0) * 255.0).round() as u8
    }

    /// Whether `alpha` sits in the "layered" range `(0, 255)`. See
    /// [`RenderAnimatedOpacity`](crate::RenderAnimatedOpacity)'s
    /// `is_layered` doc for the rationale — identical rule here.
    #[inline]
    fn is_layered(alpha: u8) -> bool {
        alpha > 0 && alpha < 255
    }

    /// Recomputes `alpha` and marks the node dirty through `handle`. See
    /// [`RenderAnimatedOpacity`](crate::RenderAnimatedOpacity)'s
    /// `recompute_alpha` for the full rule this mirrors, including why the
    /// cache commit is ordered strictly after every required mark send
    /// succeeds. Returns `true` iff alpha changed AND every required mark
    /// was sent successfully.
    fn recompute_alpha(
        animation: &ProxyAnimation<f64>,
        alpha: &AtomicU8,
        handle: &RenderInvalidationHandle,
    ) -> bool {
        let new_alpha = Self::opacity_to_alpha(animation.value());
        let old_alpha = alpha.load(Ordering::Relaxed);
        if old_alpha == new_alpha {
            return false;
        }

        if Self::is_layered(old_alpha) != Self::is_layered(new_alpha)
            && let Err(error) = handle.mark_needs_compositing_bits_update()
        {
            tracing::warn!(
                %error,
                old_alpha,
                new_alpha,
                "RenderSliverAnimatedOpacity: compositing-bits mark send \
                 failed; alpha cache left at the old value so the next tick \
                 retries"
            );
            return false;
        }

        // See `RenderAnimatedOpacity::recompute_alpha` for why this is the
        // composited-layer-update mark rather than a paint mark: an alpha-only
        // tick rebuilds just this node's `OpacityLayer` and replays the
        // enclosing boundary's retained output, and degrades to a paint mark
        // on its own when there is nothing retained to patch.
        // Crossing alpha 0 starts or stops suppressing the subtree's paint
        // entirely, and that is invisible to the layer-update path: at both
        // alpha 0 and alpha 255 no `OpacityLayer` is emitted, so there is no
        // effect-layer slot to patch. Only a repaint can add or remove that
        // content.
        //
        // Belt and braces, and knowingly so: the paint phase refuses to graft
        // when a node that requested an update owns no slot, which covers this
        // case as well — a mutation run removes this branch and the end-to-end
        // test still passes. It stays because reporting the truth here saves a
        // queue-then-refuse round trip, and because this class has already
        // produced visible corruption once.
        if (old_alpha == 0) != (new_alpha == 0) {
            if let Err(error) = handle.mark_needs_paint() {
                tracing::warn!(
                    %error,
                    old_alpha,
                    new_alpha,
                    "RenderSliverAnimatedOpacity: paint mark send failed on a visibility change; \
                     alpha cache left at the old value so the next tick retries"
                );
                return false;
            }
            alpha.store(new_alpha, Ordering::Relaxed);
            return true;
        }

        if let Err(error) = handle.mark_needs_composited_layer_update() {
            tracing::warn!(
                %error,
                old_alpha,
                new_alpha,
                "RenderSliverAnimatedOpacity: composited-layer-update mark send \
                 failed; alpha cache left at the old value so the next tick retries"
            );
            return false;
        }

        alpha.store(new_alpha, Ordering::Relaxed);
        true
    }
}

impl std::fmt::Debug for RenderSliverAnimatedOpacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderSliverAnimatedOpacity")
            .field("alpha", &self.alpha())
            .field("always_include_semantics", &self.always_include_semantics)
            .finish_non_exhaustive()
    }
}

impl flui_foundation::Diagnosticable for RenderSliverAnimatedOpacity {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        builder.add_default_double("opacity", f64::from(self.alpha()) / 255.0, 1.0, None);
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
        // Same as `RenderSliverOpacity` — hit-tests regardless of alpha.
        ctx.hit_test_child_at_layout_offset(0)
    }

    // Mirrors `RenderSliverOpacity::always_needs_compositing`: the sliver
    // compositing-bits walk reads this through
    // `dyn RenderObject<SliverProtocol>` (see `sliver/sliver_opacity.rs`'s
    // comment on that override for the walk's mechanics). The box variant,
    // `RenderAnimatedOpacity`, carries the analogous override for the same
    // reason — see its own comment for the predicate used
    // (`is_layered`, `0 < alpha < 255`) and why.
    fn always_needs_compositing(&self) -> bool {
        Self::is_layered(self.alpha())
    }

    // The whole point of this object: the pipeline reads paint_effects
    // through `&dyn RenderObject<SliverProtocol>`; the blanket impl forwards
    // here.
    fn paint_effects(&self, _size: Size) -> PaintEffects {
        let alpha = self.alpha();
        // None when fully opaque (255) or fully transparent (0): neither
        // requires an OpacityLayer.
        if alpha == 255 || alpha == 0 {
            PaintEffects::NONE
        } else {
            PaintEffects::NONE.with_opacity(PaintOpacity::new(alpha))
        }
    }

    fn skip_paint(&self) -> bool {
        self.alpha() == 0
    }

    fn attach(&mut self, handle: RenderInvalidationHandle) {
        let animation = self.animation.clone();
        let alpha = self.alpha.clone();
        let mark_handle = handle.clone();
        self.listener_id = Some(self.animation.add_listener(std::rc::Rc::new(move || {
            Self::recompute_alpha(&animation, &alpha, &mark_handle);
        })));
        Self::recompute_alpha(&self.animation, &self.alpha, &handle);
    }

    fn detach(&mut self) {
        if let Some(id) = self.listener_id.take() {
            self.animation.remove_listener(id);
        }
    }
}
