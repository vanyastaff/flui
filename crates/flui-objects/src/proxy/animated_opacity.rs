//! `RenderAnimatedOpacity` — applies a continuously-animated transparency to
//! a single child, driven by an injected [`AnimationController`].
//!
//! # Flutter equivalence
//!
//! Behavior-faithful port of Flutter's `RenderAnimatedOpacityMixin` +
//! `RenderAnimatedOpacity` (`packages/flutter/lib/src/rendering/proxy_box.dart`,
//! tag `3.44.0`). The oracle's mixin caches an `_alpha`, recomputes it on
//! every `opacity` animation tick via a listener registered in `attach`
//! (and once more, unconditionally, right after registering — "in case it
//! changed while we weren't listening"), and on each recompute: marks
//! needs-paint whenever the alpha value changed, and additionally marks
//! needs-compositing-bits whenever the recompute crosses the
//! `_alpha > 0` repaint-boundary threshold (`isRepaintBoundary` flips).
//!
//! # Documented divergence — no composited-layer update
//!
//! Flutter's mixin is a `isRepaintBoundary` node: on a tick it calls
//! `updateCompositedLayer`, which mutates the *retained* `OpacityLayer`'s
//! alpha in place, so a tick never repaints the child subtree — only the
//! compositor re-blends the cached layer. FLUI has no composited-layer-update
//! machinery (no `updateCompositedLayer`/`markNeedsCompositedLayerUpdate`
//! equivalent anywhere in `flui-rendering`/`flui-objects`), so this port
//! instead marks the node dirty for a real repaint whenever the effective
//! alpha changes, exactly like `layout::animated_size` documents its own
//! divergence at `layout/animated_size.rs:452-457`. The
//! retained-layer alpha update is Flutter's efficiency path, not yet built
//! here; a tick costs a full repaint of the subtree instead of a blend-only
//! update.
//!
//! # Zero-consumer honesty
//!
//! The `AnimatedOpacity` widget (`flui-widgets/src/animated/animated_opacity.rs`)
//! still drives its `Opacity` child through an `AnimatedBuilder` rebuild loop,
//! not through this render object. Wiring the widget to a persistent
//! render-view wrapper around `RenderAnimatedOpacity` — the pattern
//! `AnimatedSizeRenderView` establishes at
//! `flui-widgets/src/animated/animated_size.rs:239-272` — is a deliberately
//! deferred follow-up, not part of this unit.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use flui_tree::Single;
use flui_types::{Offset, Size};

use flui_animation::curve::ArcCurve;
use flui_animation::{Animation, AnimationController, CurvedAnimation};
use flui_foundation::{Listenable, ListenerId};

use flui_rendering::{
    context::{BoxHitTestContext, BoxLayoutContext},
    parent_data::BoxParentData,
    pipeline::RepaintHandle,
    traits::RenderBox,
};

/// A render object that applies a continuously-animated transparency to its
/// child.
///
/// Unlike [`RenderOpacity`](crate::RenderOpacity), the alpha is not set
/// directly — it tracks an injected [`AnimationController`] (optionally
/// eased through a [`Curve`](flui_animation::curve::Curve)) and updates
/// itself on every animation tick via a listener registered in
/// [`attach`](RenderBox::attach). See the module docs for the exact
/// dirty-marking rule ported from Flutter's `RenderAnimatedOpacityMixin`.
///
/// # Performance
///
/// See the module docs' *documented divergence* section: every alpha change
/// costs a full repaint of the subtree (FLUI has no retained-layer
/// alpha-blend update yet), not just a compositor re-blend.
pub struct RenderAnimatedOpacity {
    /// The controller driving [`animation`](Self::animation). Kept alive
    /// (and listened to in `attach`) as the sole source of opacity change —
    /// constructor-injected only, matching
    /// [`RenderAnimatedSize`](crate::RenderAnimatedSize)'s shape;
    /// there is no setter to swap it post-construction, so Flutter's
    /// `didUpdateAnimation` equivalent (`set opacity` on a live mixin) is
    /// unreachable here by construction.
    controller: AnimationController,
    /// The eased view of `controller`'s value in `[0.0, 1.0]`.
    animation: CurvedAnimation<ArcCurve>,
    /// Alpha cache (`0..=255`), shared with the tick listener closure via
    /// `Arc` so both the listener (running off the owning thread, per
    /// [`RepaintHandle`]'s cross-thread contract) and `paint_alpha`/
    /// `skip_paint` (called with `&self` from the pipeline's paint walk)
    /// observe the same up-to-date value. `AtomicU8` over `Mutex<u8>`: a
    /// single-byte cache with no compound invariant needs no lock.
    /// `Ordering::Relaxed` suffices because the dirty-channel send in
    /// [`recompute_alpha`](Self::recompute_alpha) — not the atomic op — is
    /// the synchronization edge: `alpha` is only stored *after* the mark
    /// send succeeds, so the paint walk that observes the mark is
    /// guaranteed to run after the store that produced it, via the
    /// channel's own happens-before, independent of atomic memory order.
    alpha: Arc<AtomicU8>,
    /// Whether child semantics are included regardless of alpha (Flutter
    /// parity: `alwaysIncludeSemantics`). Stored for diagnostics/API parity;
    /// this port does not yet gate `visitChildrenForSemantics` on it (no
    /// semantics-tree visitor override exists on this proxy today).
    always_include_semantics: bool,
    /// Tick-listener subscription on `controller`, torn down in `detach`.
    listener_id: Option<ListenerId>,
}

impl RenderAnimatedOpacity {
    /// Creates a render object driven by an **already-built** `controller`
    /// (this object never constructs a controller and never sees a
    /// `Vsync`/`Scheduler` — the owning view builds and registers the
    /// controller and passes it in here, matching
    /// [`RenderAnimatedSize::new`](crate::RenderAnimatedSize::new)).
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

    /// Returns the current cached alpha (`0..=255`), refreshed by the last
    /// tick that changed it (and once at construction / attach time).
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

    /// Whether `alpha` sits in the "layered" range `(0, 255)` — the exact
    /// threshold `paint_alpha`/`skip_paint` use to decide whether an
    /// `OpacityLayer` is needed. Crossing this threshold is what Flutter's
    /// mixin calls a `isRepaintBoundary` flip (`_alpha! > 0`); FLUI's own
    /// `RenderOpacity` uses the same `alpha != 255` narrowing for its
    /// no-layer fast path at full/zero opacity (see `proxy/opacity.rs`).
    #[inline]
    fn is_layered(alpha: u8) -> bool {
        alpha > 0 && alpha < 255
    }

    /// Recomputes `alpha` from `animation`'s current value and marks the
    /// node dirty through `handle` per the module docs' ported rule:
    /// needs-paint whenever alpha changed, plus needs-compositing-bits
    /// whenever the recompute crosses the [`is_layered`](Self::is_layered)
    /// threshold. Returns `true` iff alpha changed AND every required mark
    /// was sent successfully.
    ///
    /// The cache commits (`alpha.store`) only after every mark this
    /// recompute owes has been sent successfully — never before. `handle`'s
    /// send can fail under backpressure or once the pipeline owner is gone
    /// ([`SendError`]); committing first would make that failure invisible,
    /// since the next tick would then compare its new value against an
    /// already-updated cache and see no change, permanently losing the mark
    /// for whatever the animation settles on. Leaving the cache at
    /// `old_alpha` on failure means the next tick re-derives the same delta
    /// (or a larger one) against the true old value and retries the send.
    ///
    /// Free function (not `&self`) so both [`attach`](RenderBox::attach)
    /// (called with the live `self.animation`/`self.alpha`) and the tick
    /// listener closure (called with cloned copies, since the closure must
    /// be `'static` and cannot borrow `self`) share one implementation.
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
                "RenderAnimatedOpacity: compositing-bits mark send failed; \
                 alpha cache left at the old value so the next tick retries"
            );
            return false;
        }

        if let Err(error) = handle.mark_needs_paint() {
            tracing::warn!(
                %error,
                old_alpha,
                new_alpha,
                "RenderAnimatedOpacity: paint mark send failed; alpha cache \
                 left at the old value so the next tick retries"
            );
            return false;
        }

        alpha.store(new_alpha, Ordering::Relaxed);
        true
    }
}

impl std::fmt::Debug for RenderAnimatedOpacity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderAnimatedOpacity")
            .field("alpha", &self.alpha())
            .field("always_include_semantics", &self.always_include_semantics)
            .finish_non_exhaustive()
    }
}

impl flui_foundation::Diagnosticable for RenderAnimatedOpacity {
    fn debug_fill_properties(&self, properties: &mut flui_foundation::DiagnosticsBuilder) {
        properties.add_default_double("opacity", f32::from(self.alpha()) / 255.0, 1.0, None);
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

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        let constraints = *ctx.constraints();
        if ctx.child_count() > 0 {
            ctx.layout_child(0, constraints)
        } else {
            constraints.smallest()
        }
    }

    flui_rendering::forward_single_child_box_queries!();

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        // Flutter parity: an animated-opacity node does not override
        // `hitTestChildren` — it hit-tests regardless of alpha, same as
        // `RenderOpacity` (see `proxy/opacity.rs`).
        if !ctx.is_within_own_size() {
            return false;
        }
        ctx.hit_test_child_at_offset(0, Offset::ZERO)
    }

    // The whole point of this object: the pipeline reads paint_alpha through
    // `&dyn RenderObject<BoxProtocol>`; the blanket impl forwards here.
    fn paint_alpha(&self) -> Option<u8> {
        let alpha = self.alpha();
        // None when fully opaque (255) or fully transparent (0): neither
        // requires an OpacityLayer. Flutter: alpha=0 -> layer=null.
        if alpha == 255 || alpha == 0 {
            None
        } else {
            Some(alpha)
        }
    }

    fn skip_paint(&self) -> bool {
        // Flutter RenderAnimatedOpacityMixin.paint: `if (_alpha == 0) return;`
        self.alpha() == 0
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
    // Flutter parity: `RenderAnimatedOpacityMixin.isRepaintBoundary`
    // (`proxy_box.dart:985`) = `child != null && _currentlyIsRepaintBoundary`
    // where `_currentlyIsRepaintBoundary = _alpha! > 0` — Flutter's own
    // threshold is plain `alpha > 0` (fully opaque still counts as
    // needing its own layer). This port instead uses
    // [`is_layered`](Self::is_layered) (`0 < alpha < 255`), the SAME
    // predicate `paint_alpha`/`skip_paint` above already use, and the one
    // `RenderOpacity`/`RenderSliverOpacity` establish for this crate: at
    // `alpha == 255` no layer is ever allocated (`paint_alpha` returns
    // `None`), so requiring compositing there would be pure overhead with
    // no visual effect — consistency with the sibling opacity pair's
    // predicate wins over a literal transcription of Flutter's threshold.
    //
    // `RenderOpacity` (the non-animated box sibling) has no analogous
    // override at all — a separate, pre-existing gap in this crate that
    // predates this change and stays out of scope here.
    fn always_needs_compositing(&self) -> bool {
        Self::is_layered(self.alpha())
    }

    fn attach(&mut self, handle: RepaintHandle) {
        let animation = self.animation.clone();
        let alpha = self.alpha.clone();
        let mark_handle = handle.clone();
        self.listener_id = Some(self.controller.add_listener(Arc::new(move || {
            Self::recompute_alpha(&animation, &alpha, &mark_handle);
        })));

        // Oracle (`proxy_box.dart` attach): `opacity.addListener(_updateOpacity);
        // _updateOpacity();` — refresh once more in case the animation's value
        // changed while this node wasn't listening (e.g. a detach/reattach).
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
    use flui_rendering::protocol::BoxProtocol;
    use std::time::Duration;

    fn controller(ms: u64) -> AnimationController {
        AnimationController::new(Duration::from_millis(ms), Arc::new(Scheduler::new()))
    }

    fn render_at(opacity: f32) -> RenderAnimatedOpacity {
        let c = controller(100);
        c.set_value(opacity);
        RenderAnimatedOpacity::new(c, ArcCurve::new(Curves::Linear), false)
    }

    fn curved_at(opacity: f32) -> CurvedAnimation<ArcCurve> {
        let c = controller(100);
        c.set_value(opacity);
        let parent: Arc<dyn Animation<f32>> = Arc::new(c);
        CurvedAnimation::new(parent, ArcCurve::new(Curves::Linear))
    }

    /// Mints a real [`RepaintHandle`] by inserting a throwaway anchor node —
    /// `RepaintHandle::new` is `pub(super)` to `flui_rendering::pipeline`, so
    /// a real one can only come from a live `PipelineOwner` (matches
    /// `RenderAnimatedSize`'s own tests, which go through `PipelineOwner::insert`
    /// rather than constructing a handle by hand).
    fn anchor_handle() -> (PipelineOwner, RepaintHandle) {
        let mut owner = PipelineOwner::new();
        let anchor = owner
            .insert(Box::new(render_at(1.0))
                as Box<dyn flui_rendering::traits::RenderObject<BoxProtocol>>);
        let handle = owner
            .repaint_handle(anchor)
            .expect("just-inserted id must be live");
        (owner, handle)
    }

    #[test]
    fn new_caches_alpha_from_initial_animation_value() {
        let ro = render_at(0.5);
        assert_eq!(ro.alpha(), 128);
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
        assert!(!render_at(0.5).skip_paint());
    }

    #[test]
    fn is_layered_is_false_exactly_at_0_and_255() {
        assert!(!RenderAnimatedOpacity::is_layered(0));
        assert!(!RenderAnimatedOpacity::is_layered(255));
        assert!(RenderAnimatedOpacity::is_layered(1));
        assert!(RenderAnimatedOpacity::is_layered(254));
    }

    // `recompute_alpha` is the function attach's listener closure delegates
    // to. These three tests pin its dirty-marking decision table directly —
    // no-op on an unchanged value, paint-only on a same-bucket change,
    // paint+compositing-bits on a boundary-crossing change — independent of
    // the pipeline wiring (the harness file proves the wiring separately).
    #[test]
    fn recompute_alpha_reports_no_change_when_value_is_unchanged() {
        let (mut owner, handle) = anchor_handle();
        let cache = AtomicU8::new(128);

        // `insert()` already marks a fresh node needing paint (every new node
        // needs its first paint) — baseline before asserting `recompute_alpha`
        // adds nothing further.
        owner.drain_pending_dirty();
        let paint_dirty_before = owner.nodes_needing_paint().len();

        let changed = RenderAnimatedOpacity::recompute_alpha(&curved_at(0.5), &cache, &handle);

        assert!(
            !changed,
            "recomputing the same alpha (128, from opacity 0.5) must report no change"
        );
        owner.drain_pending_dirty();
        assert_eq!(
            owner.nodes_needing_paint().len(),
            paint_dirty_before,
            "an unchanged alpha must not add a new paint-dirty entry"
        );
    }

    // Proves the commit-ordering fix: when the mark send fails, the cache
    // must NOT advance, so a later successful tick still sees the true
    // delta and retries the mark instead of silently losing it. Dropping
    // `owner` closes the dirty-request channel's receiver, so any further
    // send on `handle` returns `SendError::OwnerGone` — a real, cheaply
    // reproducible failure path (not a channel-full simulation, but exactly
    // the same code path: `RepaintHandle::mark_needs_paint`/
    // `mark_needs_compositing_bits_update` returning `Err`).
    #[test]
    fn recompute_alpha_does_not_advance_the_cache_when_the_mark_send_fails() {
        let (owner, handle) = anchor_handle();
        drop(owner);

        // 0.0 -> 0.5 crosses the layered boundary, so the compositing-bits
        // send is attempted first; it fails immediately (owner gone) and
        // the function must return before ever touching the cache or
        // attempting the paint-mark send.
        let cache = AtomicU8::new(0);
        let changed = RenderAnimatedOpacity::recompute_alpha(&curved_at(0.5), &cache, &handle);

        assert!(
            !changed,
            "a failed mark send must report no committed change"
        );
        assert_eq!(
            cache.load(Ordering::Relaxed),
            0,
            "the cache must stay at the old value when the send fails, so \
             the next tick re-derives the same delta against the true old \
             value and retries the mark instead of losing it"
        );
    }

    #[test]
    fn recompute_alpha_marks_paint_only_within_the_same_layered_bucket() {
        let (mut owner, handle) = anchor_handle();
        let anchor = handle.id();
        let cache = AtomicU8::new(RenderAnimatedOpacity::opacity_to_alpha(0.4));

        // 0.4 -> 0.6: both land in the open (0, 255) layered range, so no
        // repaint-boundary threshold is crossed.
        let changed = RenderAnimatedOpacity::recompute_alpha(&curved_at(0.6), &cache, &handle);
        assert!(changed);

        owner.drain_pending_dirty();
        assert!(
            owner
                .nodes_needing_paint()
                .iter()
                .any(|dirty| dirty.id == anchor),
            "a same-bucket alpha change must still mark paint dirty"
        );
        assert!(
            owner
                .nodes_needing_compositing_bits_update()
                .iter()
                .all(|dirty| dirty.id != anchor),
            "a same-bucket alpha change must NOT mark compositing bits dirty"
        );
    }

    #[test]
    fn recompute_alpha_marks_compositing_bits_when_crossing_the_layered_boundary() {
        let (mut owner, handle) = anchor_handle();
        let anchor = handle.id();
        let cache = AtomicU8::new(0); // starts fully transparent — not layered

        // 0 -> 0.5 (alpha 128): crosses into the layered range.
        let changed = RenderAnimatedOpacity::recompute_alpha(&curved_at(0.5), &cache, &handle);
        assert!(changed);

        owner.drain_pending_dirty();
        assert!(
            owner
                .nodes_needing_paint()
                .iter()
                .any(|dirty| dirty.id == anchor),
            "a boundary-crossing alpha change must mark paint dirty"
        );
        assert!(
            owner
                .nodes_needing_compositing_bits_update()
                .iter()
                .any(|dirty| dirty.id == anchor),
            "crossing the layered/unlayered threshold must mark compositing bits dirty"
        );
    }

    // attach must register a listener and detach must clear it — a
    // white-box assertion on the private `listener_id` field is the most
    // direct proof available (a real `PipelineOwner::remove_render_object`
    // detach cannot isolate "listener removed" from "id went stale", since
    // both share the same observable no-op — see that method's own doc).
    #[test]
    fn attach_registers_listener_and_detach_clears_it() {
        let (_owner, handle) = anchor_handle();
        let mut ro = render_at(0.5);
        assert!(ro.listener_id.is_none(), "no listener before attach");

        RenderBox::attach(&mut ro, handle);
        assert!(ro.listener_id.is_some(), "attach must register a listener");

        RenderBox::detach(&mut ro);
        assert!(
            ro.listener_id.is_none(),
            "detach must clear the listener id"
        );
    }
}
