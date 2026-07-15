//! `RenderSliverAnimatedOpacity` — applies a continuously-animated
//! transparency to a single sliver child, driven by an injected
//! [`AnimationController`].
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's `RenderAnimatedOpacityMixin` +
//! `RenderSliverAnimatedOpacity` (`packages/flutter/lib/src/rendering/proxy_sliver.dart`,
//! tag `3.44.0`). `RenderSliverAnimatedOpacity` is a bare
//! `RenderProxySliver with RenderAnimatedOpacityMixin<RenderSliver>` — it
//! mixes in the SAME alpha-caching/dirty-marking rule the box variant uses.
//! See [`RenderAnimatedOpacity`](crate::RenderAnimatedOpacity)'s module docs
//! for the full ported-mechanism writeup (alpha caching, the
//! paint/compositing-bits marking rule, the documented
//! no-composited-layer-update divergence, and the `is_layered` predicate
//! both variants use for `always_needs_compositing`) — this module only
//! restates the sliver-specific contract surface (`RenderSliverOpacity`'s
//! layout/hit-test passthrough) that the mixin's host class supplies.
//!
//! # Zero-consumer honesty
//!
//! No widget in `flui-widgets` constructs this render object yet; wiring a
//! sliver-flavored `AnimatedOpacity` to it is a deferred follow-up, same as
//! the box variant.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use flui_tree::Single;

use flui_animation::curve::ArcCurve;
use flui_animation::{Animation, AnimationController, CurvedAnimation};
use flui_foundation::{Listenable, ListenerId};

use flui_rendering::{
    constraints::SliverGeometry,
    context::{SliverHitTestContext, SliverLayoutContext},
    parent_data::SliverPhysicalParentData,
    pipeline::RepaintHandle,
    traits::RenderSliver,
};

/// A sliver render object that applies a continuously-animated transparency
/// to its single sliver child.
///
/// Mirrors [`RenderAnimatedOpacity`](crate::RenderAnimatedOpacity) over
/// [`RenderSliverOpacity`](crate::RenderSliverOpacity)'s contract instead of
/// `RenderOpacity`'s. See the module docs.
pub struct RenderSliverAnimatedOpacity {
    /// The controller driving [`animation`](Self::animation).
    /// Constructor-injected only — see
    /// [`RenderAnimatedOpacity`](crate::RenderAnimatedOpacity)'s field
    /// doc for why no post-construction swap setter exists.
    controller: AnimationController,
    /// The eased view of `controller`'s value in `[0.0, 1.0]`.
    animation: CurvedAnimation<ArcCurve>,
    /// Alpha cache (`0..=255`), shared with the tick listener closure.
    /// See [`RenderAnimatedOpacity`](crate::RenderAnimatedOpacity)'s
    /// field doc for why this is an `AtomicU8`, not a `Cell`/`Mutex`, and
    /// why `Ordering::Relaxed` suffices (the dirty-channel send in
    /// [`recompute_alpha`](Self::recompute_alpha), not the atomic op, is
    /// the synchronization edge).
    alpha: Arc<AtomicU8>,
    /// Whether child semantics are included regardless of alpha.
    always_include_semantics: bool,
    /// Tick-listener subscription on `controller`, torn down in `detach`.
    listener_id: Option<ListenerId>,
}

impl RenderSliverAnimatedOpacity {
    /// Creates a render object driven by an **already-built** `controller`
    /// (never constructs one itself — see
    /// [`RenderAnimatedOpacity::new`](crate::RenderAnimatedOpacity::new)).
    #[must_use]
    pub fn new(
        controller: AnimationController,
        curve: ArcCurve,
        always_include_semantics: bool,
    ) -> Self {
        let parent: Arc<dyn Animation<f32>> = Arc::new(controller.clone());
        let animation = CurvedAnimation::new(parent, curve);
        let alpha = Arc::new(AtomicU8::new(Self::opacity_to_alpha(animation.value())));
        Self {
            controller,
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

    /// Whether child semantics are included regardless of alpha.
    #[inline]
    #[must_use]
    pub fn always_include_semantics(&self) -> bool {
        self.always_include_semantics
    }

    /// Converts opacity (`0.0..=1.0`) to alpha (`0..=255`).
    #[inline]
    fn opacity_to_alpha(opacity: f32) -> u8 {
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
        animation: &CurvedAnimation<ArcCurve>,
        alpha: &AtomicU8,
        handle: &RepaintHandle,
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

        if let Err(error) = handle.mark_needs_paint() {
            tracing::warn!(
                %error,
                old_alpha,
                new_alpha,
                "RenderSliverAnimatedOpacity: paint mark send failed; alpha \
                 cache left at the old value so the next tick retries"
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
        builder.add_default_double("opacity", f32::from(self.alpha()) / 255.0, 1.0, None);
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
        // Flutter parity: same as `RenderSliverOpacity` — hit-tests
        // regardless of alpha.
        ctx.hit_test_child_at_layout_offset(0)
    }

    // Mirrors `RenderSliverOpacity::always_needs_compositing`: the sliver
    // compositing-bits walk reads this through
    // `dyn RenderObject<SliverProtocol>` (see `sliver/sliver_opacity.rs`'s
    // comment on that override for the walk's mechanics). The box variant,
    // `RenderAnimatedOpacity`, carries the analogous override for the same
    // reason — see its own comment for the predicate this port uses
    // (`is_layered`, `0 < alpha < 255`) and why, versus Flutter's raw
    // `alpha > 0`.
    fn always_needs_compositing(&self) -> bool {
        Self::is_layered(self.alpha())
    }

    fn paint_alpha(&self) -> Option<u8> {
        let alpha = self.alpha();
        if alpha == 255 || alpha == 0 {
            None
        } else {
            Some(alpha)
        }
    }

    fn skip_paint(&self) -> bool {
        self.alpha() == 0
    }

    fn attach(&mut self, handle: RepaintHandle) {
        let animation = self.animation.clone();
        let alpha = self.alpha.clone();
        let mark_handle = handle.clone();
        self.listener_id = Some(self.controller.add_listener(Arc::new(move || {
            Self::recompute_alpha(&animation, &alpha, &mark_handle);
        })));
        Self::recompute_alpha(&self.animation, &self.alpha, &handle);
    }

    fn detach(&mut self) {
        if let Some(id) = self.listener_id.take() {
            self.controller.remove_listener(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_animation::{Curves, Scheduler};
    use flui_rendering::pipeline::PipelineOwner;
    use flui_rendering::protocol::SliverProtocol;
    use std::time::Duration;

    fn controller(ms: u64) -> AnimationController {
        AnimationController::new(Duration::from_millis(ms), Arc::new(Scheduler::new()))
    }

    fn render_at(opacity: f32) -> RenderSliverAnimatedOpacity {
        let c = controller(100);
        c.set_value(opacity);
        RenderSliverAnimatedOpacity::new(c, ArcCurve::new(Curves::Linear), false)
    }

    fn curved_at(opacity: f32) -> CurvedAnimation<ArcCurve> {
        let c = controller(100);
        c.set_value(opacity);
        let parent: Arc<dyn Animation<f32>> = Arc::new(c);
        CurvedAnimation::new(parent, ArcCurve::new(Curves::Linear))
    }

    fn anchor_handle() -> (PipelineOwner, RepaintHandle) {
        let mut owner = PipelineOwner::new();
        let anchor = owner.insert(Box::new(render_at(1.0))
            as Box<dyn flui_rendering::traits::RenderObject<SliverProtocol>>);
        let handle = owner
            .repaint_handle(anchor)
            .expect("just-inserted id must be live");
        (owner, handle)
    }

    #[test]
    fn new_caches_alpha_from_initial_animation_value() {
        assert_eq!(render_at(0.5).alpha(), 128);
    }

    #[test]
    fn paint_alpha_returns_none_when_opaque_or_transparent() {
        assert_eq!(render_at(1.0).paint_alpha(), None);
        assert_eq!(render_at(0.0).paint_alpha(), None);
    }

    #[test]
    fn paint_alpha_returns_some_for_partial() {
        assert_eq!(render_at(0.5).paint_alpha(), Some(128));
    }

    #[test]
    fn skip_paint_true_only_when_fully_transparent() {
        assert!(render_at(0.0).skip_paint());
        assert!(!render_at(1.0).skip_paint());
    }

    #[test]
    fn always_needs_compositing_tracks_the_layered_range() {
        assert!(!render_at(0.0).always_needs_compositing());
        assert!(!render_at(1.0).always_needs_compositing());
        assert!(render_at(0.5).always_needs_compositing());
    }

    #[test]
    fn recompute_alpha_reports_no_change_when_value_is_unchanged() {
        let (mut owner, handle) = anchor_handle();
        let cache = AtomicU8::new(128);

        // `insert()` already marks a fresh node needing paint — baseline
        // before asserting `recompute_alpha` adds nothing further.
        owner.drain_pending_dirty();
        let paint_dirty_before = owner.nodes_needing_paint().len();

        let changed =
            RenderSliverAnimatedOpacity::recompute_alpha(&curved_at(0.5), &cache, &handle);

        assert!(!changed);
        owner.drain_pending_dirty();
        assert_eq!(
            owner.nodes_needing_paint().len(),
            paint_dirty_before,
            "an unchanged alpha must not add a new paint-dirty entry"
        );
    }

    // Mirrors the box variant's failed-send test: dropping `owner` closes
    // the dirty-request channel's receiver, so `handle`'s send returns
    // `SendError::OwnerGone` — a real, cheaply reproducible failure of the
    // exact code path `recompute_alpha` calls. The cache must stay at the
    // old value so a later successful tick still re-derives the delta and
    // retries the mark instead of losing it.
    #[test]
    fn recompute_alpha_does_not_advance_the_cache_when_the_mark_send_fails() {
        let (owner, handle) = anchor_handle();
        drop(owner);

        let cache = AtomicU8::new(0);
        let changed =
            RenderSliverAnimatedOpacity::recompute_alpha(&curved_at(0.5), &cache, &handle);

        assert!(
            !changed,
            "a failed mark send must report no committed change"
        );
        assert_eq!(
            cache.load(Ordering::Relaxed),
            0,
            "the cache must stay at the old value when the send fails"
        );
    }

    #[test]
    fn recompute_alpha_marks_compositing_bits_when_crossing_the_layered_boundary() {
        let (mut owner, handle) = anchor_handle();
        let anchor = handle.id();
        let cache = AtomicU8::new(0);

        let changed =
            RenderSliverAnimatedOpacity::recompute_alpha(&curved_at(0.5), &cache, &handle);
        assert!(changed);

        owner.drain_pending_dirty();
        assert!(
            owner
                .nodes_needing_paint()
                .iter()
                .any(|dirty| dirty.id == anchor)
        );
        assert!(
            owner
                .nodes_needing_compositing_bits_update()
                .iter()
                .any(|dirty| dirty.id == anchor),
            "crossing the layered/unlayered threshold must mark compositing bits dirty"
        );
    }

    #[test]
    fn recompute_alpha_marks_paint_only_within_the_same_layered_bucket() {
        let (mut owner, handle) = anchor_handle();
        let anchor = handle.id();
        let cache = AtomicU8::new(RenderSliverAnimatedOpacity::opacity_to_alpha(0.4));

        let changed =
            RenderSliverAnimatedOpacity::recompute_alpha(&curved_at(0.6), &cache, &handle);
        assert!(changed);

        owner.drain_pending_dirty();
        assert!(
            owner
                .nodes_needing_compositing_bits_update()
                .iter()
                .all(|dirty| dirty.id != anchor),
            "a same-bucket alpha change must NOT mark compositing bits dirty"
        );
    }

    #[test]
    fn attach_registers_listener_and_detach_clears_it() {
        let (_owner, handle) = anchor_handle();
        let mut ro = render_at(0.5);
        assert!(ro.listener_id.is_none());

        RenderSliver::attach(&mut ro, handle);
        assert!(ro.listener_id.is_some());

        RenderSliver::detach(&mut ro);
        assert!(ro.listener_id.is_none());
    }
}
