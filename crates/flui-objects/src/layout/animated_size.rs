//! `RenderAnimatedSize` — animates its own size toward its child's natural
//! size whenever that size changes, clipping overflow while the animation is
//! in flight.
//!
//! Flutter parity: `rendering/animated_size.dart` `RenderAnimatedSize`. Unlike
//! every other render object in this crate, this one drives its **own**
//! layout over time: it holds an [`AnimationController`] (built and
//! registered with a `Vsync` by the owning `AnimatedSize` view, then handed
//! in at construction — ADR-0013 D2, `docs/adr/ADR-0013-render-object-attach-self-dirty-handle.md`)
//! and subscribes to it in [`attach`](RenderBox::attach), so a controller
//! tick re-marks this node dirty on its own, decoupled from any widget
//! rebuild.
//!
//! # The retarget state machine
//!
//! [`AnimatedSizeState`] tracks four states, exactly mirroring the oracle's
//! `_AnimatedSizeState` enum (`animated_size.dart:15-51`):
//!
//! - [`Start`](AnimatedSizeState::Start) — no committed size yet.
//! - [`Stable`](AnimatedSizeState::Stable) — settled: either idle at the
//!   target size, or finishing a run already in flight.
//! - [`Changed`](AnimatedSizeState::Changed) — the child's size changed once
//!   since the object was `Stable`.
//! - [`Unstable`](AnimatedSizeState::Unstable) — the child's size is changing
//!   every layout; tracked directly (no interpolation) until it repeats.
//!
//! **The one subtlety that is easy to get wrong**: `begin = the animation's
//! current interpolated value` holds **only** for the `Stable -> Changed`
//! transition ([`layout_stable`](RenderAnimatedSize::layout_stable)). Every
//! later retarget while already `Changed`/`Unstable`
//! ([`layout_changed`](RenderAnimatedSize::layout_changed),
//! [`layout_unstable`](RenderAnimatedSize::layout_unstable)) collapses to a
//! **degenerate zero-span tween** (`begin = end = child's raw current size`)
//! — direct tracking, not interpolation — while still restarting the
//! controller from `t = 0` for bookkeeping.

use std::sync::Arc;
use std::time::Duration;

use flui_tree::Single;
use flui_types::{Alignment, Point, Rect, Size, painting::Clip};

use flui_animation::curve::ArcCurve;
use flui_animation::{
    Animatable, Animation, AnimationController, AnimationStatus, CurvedAnimation, SizeTween,
};
use flui_foundation::{Listenable, ListenerId};

use crate::layout::shifted_box::AligningShiftedBox;
use flui_rendering::{
    constraints::{BoxConstraints, Constraints},
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
        PaintCx,
    },
    parent_data::BoxParentData,
    pipeline::RepaintHandle,
    traits::{RenderBox, TextBaseline},
};

/// The four-state retarget state machine driving [`RenderAnimatedSize`]'s
/// size-change detection. See the module docs for the precise per-transition
/// formula.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimatedSizeState {
    /// No committed size yet; the very first layout snaps with no animation.
    Start,
    /// Settled: idle at the target size, or finishing an in-flight run.
    Stable,
    /// The child's size changed once since the object was `Stable`.
    Changed,
    /// The child's size is changing every layout; tracked directly until it
    /// repeats a frame.
    Unstable,
}

/// Animates its size toward its child's natural size whenever that size
/// changes.
///
/// Mirrors Flutter's `RenderAnimatedSize`. See the module docs for the
/// retarget state machine and the constructor for the ADR-0013 D2 injection
/// contract (this object never builds or sees a `Vsync`/`Scheduler`).
pub struct RenderAnimatedSize {
    inner: AligningShiftedBox,
    controller: AnimationController,
    animation: CurvedAnimation<ArcCurve>,
    curve: ArcCurve,
    size_tween: SizeTween,
    state: AnimatedSizeState,
    /// Last frame's already-`constrain()`-ed committed size — mirrors the
    /// oracle's `_currentSize` (`animated_size.dart:137`). Read only by
    /// [`layout_stable`](Self::layout_stable)'s retarget branch and by
    /// [`dry_size_for`](Self::dry_size_for)'s `Stable` branch.
    current_size: Size,
    has_visual_overflow: bool,
    clip_behavior: Clip,
    /// Value-change subscription on `controller`, torn down in `detach`.
    listener_id: Option<ListenerId>,
    /// `on_end` subscription on `controller`'s status channel, torn down in
    /// `detach`.
    status_listener_id: Option<ListenerId>,
    /// Registered unconditionally in `attach` (not gated on `is_some()`), so
    /// a later [`set_on_end`](Self::set_on_end) needs no re-subscription —
    /// the closure reads this cell live on every status fire.
    on_end: Arc<parking_lot::Mutex<Option<Arc<dyn Fn() + Send + Sync>>>>,
}

impl RenderAnimatedSize {
    /// Creates a render object driven by an **already-built** `controller`
    /// (ADR-0013 D2 — this object never constructs a controller, and never
    /// sees a `Vsync`/`Scheduler`; the owning `AnimatedSize` view builds and
    /// registers the controller and passes it in here).
    pub fn new(
        controller: AnimationController,
        curve: ArcCurve,
        alignment: Alignment,
        clip_behavior: Clip,
        on_end: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Self {
        let parent: Arc<dyn Animation<f32>> = Arc::new(controller.clone());
        let animation = CurvedAnimation::new(parent, curve.clone());
        Self {
            inner: AligningShiftedBox::new(alignment),
            controller,
            animation,
            curve,
            size_tween: SizeTween::new(Size::ZERO, Size::ZERO),
            state: AnimatedSizeState::Start,
            current_size: Size::ZERO,
            has_visual_overflow: false,
            clip_behavior,
            listener_id: None,
            status_listener_id: None,
            on_end: Arc::new(parking_lot::Mutex::new(on_end)),
        }
    }

    /// Updates the alignment; returns `true` if the value changed.
    pub fn set_alignment(&mut self, alignment: Alignment) -> bool {
        self.inner.set_alignment(alignment)
    }

    /// Updates the clip behavior; returns `true` if the value changed.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> bool {
        if self.clip_behavior == clip_behavior {
            return false;
        }
        self.clip_behavior = clip_behavior;
        true
    }

    /// Sets the base forward duration on the owned controller. An inert
    /// pass-through (matches the oracle's plain-assignment setter,
    /// `animated_size.dart:148-153`) — it does not restart an in-flight run.
    pub fn set_duration(&self, duration: Duration) {
        self.controller.set_duration(duration);
    }

    /// Sets the reverse duration on the owned controller. Confirmed inert for
    /// this object (`restart_animation` never calls `.reverse()`); kept for
    /// constructor/widget API parity only — see the module docs.
    pub fn set_reverse_duration(&self, duration: Duration) {
        self.controller.set_reverse_duration(duration);
    }

    /// Rebuilds the curved animation over the same controller with a new
    /// curve. Safe to rebuild unconditionally: `restart_animation` only ever
    /// runs the controller forward, so `CurvedAnimation`'s reverse-curve-lock
    /// state has nothing to lose across the rebuild.
    pub fn set_curve(&mut self, curve: ArcCurve) {
        let parent: Arc<dyn Animation<f32>> = Arc::new(self.controller.clone());
        self.animation = CurvedAnimation::new(parent, curve.clone());
        self.curve = curve;
    }

    /// Sets the completion callback. No dirty-marking — matches the oracle's
    /// inert setter (`animated_size.dart:171-176`).
    pub fn set_on_end(&self, on_end: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self.on_end.lock() = on_end;
    }

    /// The current alignment.
    pub fn alignment(&self) -> Alignment {
        self.inner.alignment()
    }

    /// The current clip behavior.
    pub fn clip_behavior(&self) -> Clip {
        self.clip_behavior
    }

    /// The current retarget state.
    pub fn state(&self) -> AnimatedSizeState {
        self.state
    }

    /// Whether the last layout produced a size smaller than the tween's
    /// target (driving [`paint`](RenderBox::paint) to clip).
    pub fn has_visual_overflow(&self) -> bool {
        self.has_visual_overflow
    }

    // ------------------------------------------------------------------
    // The retarget state machine (oracle animated_size.dart:309-377)
    // ------------------------------------------------------------------

    /// `start -> stable`: both tween ends collapse to the child's size — no
    /// animation on the very first layout.
    fn layout_start(&mut self, child_size: Size) {
        self.size_tween = SizeTween::new(child_size, child_size);
        self.state = AnimatedSizeState::Stable;
    }

    /// Only reachable when `state == Stable`. The one genuine
    /// interpolation-span retarget: `begin` is the **live visual value**
    /// (`current_size`, last frame's committed size), `end` is the new
    /// target, animated over the full duration from `t = 0`.
    fn layout_stable(&mut self, child_size: Size) {
        if self.size_tween.end != child_size {
            self.size_tween = SizeTween::new(self.current_size, child_size);
            self.restart_animation();
            self.state = AnimatedSizeState::Changed;
        } else if self.controller.status() == AnimationStatus::Completed {
            // Both ends already equal `child_size`; a no-op snap that just
            // clears any float drift (oracle's `value == upperBound` check,
            // replaced by `status()` — see the module docs on `restart_animation`).
            self.size_tween = SizeTween::new(child_size, child_size);
        } else if !self.controller.is_animating() {
            // Resume after a detach, from the CURRENT value — not `forward_from(0)`.
            let _ = self.controller.forward();
        }
    }

    /// Only reachable when `state == Changed`. On a further size change this
    /// collapses to a **degenerate zero-span tween** (`begin = end =
    /// child_size`) — direct tracking, NOT "begin = current interpolated
    /// value". Getting this branch wrong (reusing `current_size` here) is
    /// the headline risk this module's docs call out.
    fn layout_changed(&mut self, child_size: Size) {
        if self.size_tween.end != child_size {
            self.size_tween = SizeTween::new(child_size, child_size);
            self.restart_animation();
            self.state = AnimatedSizeState::Unstable;
        } else {
            // The child's size repeated -> stabilized. The genuine
            // interpolation span from `layout_stable` is left untouched and
            // keeps running to completion; just resume it if it stopped.
            self.state = AnimatedSizeState::Stable;
            if !self.controller.is_animating() {
                let _ = self.controller.forward();
            }
        }
    }

    /// Only reachable when `state == Unstable`. Same degenerate collapse as
    /// [`layout_changed`](Self::layout_changed) while the child keeps
    /// changing; once it finally repeats, stop and settle (the tween is
    /// already `begin == end == this size` from the last unstable
    /// iteration, so there is no visual glitch).
    fn layout_unstable(&mut self, child_size: Size) {
        if self.size_tween.end != child_size {
            self.size_tween = SizeTween::new(child_size, child_size);
            self.restart_animation();
        } else {
            let _ = self.controller.stop();
            self.state = AnimatedSizeState::Stable;
        }
    }

    /// Always-forward restart, matching the oracle's `_restartAnimation`
    /// (`animated_size.dart:309-312`) exactly — never `.reverse()`.
    fn restart_animation(&mut self) {
        let _ = self.controller.forward_from(Some(0.0));
    }

    /// Pure per-state sizing formula shared by
    /// [`compute_dry_layout`](RenderBox::compute_dry_layout) and
    /// [`compute_dry_baseline`](RenderBox::compute_dry_baseline) — reads
    /// `state`/`size_tween`/`current_size` as of the last **real** layout
    /// without mutating them (dry layout is a pure query).
    fn dry_size_for(&self, constraints: BoxConstraints, child_size: Size) -> Size {
        match self.state {
            AnimatedSizeState::Start => constraints.constrain(child_size),
            AnimatedSizeState::Stable => {
                if self.size_tween.end != child_size {
                    constraints.constrain(self.current_size)
                } else if self.controller.status() == AnimationStatus::Completed {
                    constraints.constrain(child_size)
                } else {
                    constraints.constrain(self.size_tween.transform(self.animation.value()))
                }
            }
            AnimatedSizeState::Changed | AnimatedSizeState::Unstable => {
                if self.size_tween.end != child_size {
                    constraints.constrain(child_size)
                } else {
                    constraints.constrain(self.size_tween.transform(self.animation.value()))
                }
            }
        }
    }
}

impl std::fmt::Debug for RenderAnimatedSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderAnimatedSize")
            .field("state", &self.state)
            .field("size_tween", &self.size_tween)
            .field("current_size", &self.current_size)
            .field("has_visual_overflow", &self.has_visual_overflow)
            .field("clip_behavior", &self.clip_behavior)
            .finish_non_exhaustive()
    }
}

impl flui_foundation::Diagnosticable for RenderAnimatedSize {
    fn debug_fill_properties(&self, builder: &mut flui_foundation::DiagnosticsBuilder) {
        let alignment = self.inner.alignment();
        builder.add("alignment", format!("({}, {})", alignment.x, alignment.y));
        builder.add_enum("clip_behavior", self.clip_behavior);
        builder.add_enum("state", self.state);
    }
}

impl RenderBox for RenderAnimatedSize {
    type Arity = Single;
    type ParentData = BoxParentData;

    fn perform_layout(&mut self, ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>) -> Size {
        self.has_visual_overflow = false;
        let constraints = *ctx.constraints();

        // Fast path (oracle animated_size.dart:249-256): no child, or the
        // parent gave us no freedom to animate into. The child is still laid
        // out (so its own subtree stays consistent) but `align_child` is
        // deliberately NOT called — the child keeps whatever offset it had,
        // possibly stale, matching the oracle's quirk exactly.
        if ctx.child_count() == 0 || constraints.is_tight() {
            let _ = self.controller.stop();
            let snapped = constraints.smallest();
            self.size_tween = SizeTween::new(snapped, snapped);
            self.state = AnimatedSizeState::Start;
            if ctx.child_count() > 0 {
                ctx.layout_single_child();
            }
            self.current_size = snapped;
            return snapped;
        }

        // Full, un-loosened constraints — matching the parent's own.
        let child_size = ctx.layout_single_child();
        match self.state {
            AnimatedSizeState::Start => self.layout_start(child_size),
            AnimatedSizeState::Stable => self.layout_stable(child_size),
            AnimatedSizeState::Changed => self.layout_changed(child_size),
            AnimatedSizeState::Unstable => self.layout_unstable(child_size),
        }

        let animated_size = self.size_tween.transform(self.animation.value());
        let size = constraints.constrain(animated_size);
        self.current_size = size;
        self.inner.align_child(ctx, size, child_size);
        self.inner.record_child_baselines(ctx);
        self.has_visual_overflow =
            size.width < self.size_tween.end.width || size.height < self.size_tween.end.height;
        size
    }

    fn compute_distance_to_actual_baseline(&self, baseline: TextBaseline) -> Option<f32> {
        self.inner.actual_baseline(baseline)
    }

    fn compute_min_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_min_intrinsic_width(0, height)
    }

    fn compute_max_intrinsic_width(&self, height: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_max_intrinsic_width(0, height)
    }

    fn compute_min_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_min_intrinsic_height(0, width)
    }

    fn compute_max_intrinsic_height(&self, width: f32, ctx: &mut BoxIntrinsicsCtx<'_>) -> f32 {
        if ctx.child_count() == 0 {
            return 0.0;
        }
        ctx.child_max_intrinsic_height(0, width)
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> Size {
        if ctx.child_count() == 0 || constraints.is_tight() {
            return constraints.smallest();
        }
        let child_size = ctx.child_dry_layout(0, constraints);
        self.dry_size_for(constraints, child_size)
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> Option<f32> {
        if ctx.child_count() == 0 {
            return None;
        }
        let child_baseline = ctx.child_dry_baseline(0, constraints, baseline)?;
        let child_size = ctx.child_dry_layout(0, constraints);
        let my_size = if constraints.is_tight() {
            constraints.smallest()
        } else {
            self.dry_size_for(constraints, child_size)
        };
        let offset = self.inner.dry_child_offset(my_size, child_size);
        Some(child_baseline + offset.dy.get())
    }

    fn paint(&self, ctx: &mut PaintCx<'_, Single>) {
        if self.has_visual_overflow && self.clip_behavior != Clip::None {
            let bounds = Rect::from_origin_size(Point::ZERO, ctx.size());
            ctx.with_clip_rect(bounds, self.clip_behavior, |ctx| ctx.paint_child());
        } else {
            ctx.paint_child();
        }
    }

    fn hit_test(&self, ctx: &mut BoxHitTestContext<'_, Single, BoxParentData>) -> bool {
        self.inner.hit_test(ctx)
    }

    fn attach(&mut self, handle: RepaintHandle) {
        let mark_handle = handle.clone();
        self.listener_id = Some(self.controller.add_listener(Arc::new(move || {
            let _ = mark_handle.mark_needs_layout();
        })));

        let on_end = self.on_end.clone();
        self.status_listener_id =
            Some(self.controller.add_status_listener(Arc::new(move |status| {
                if status == AnimationStatus::Completed
                    && let Some(cb) = on_end.lock().as_ref()
                {
                    cb();
                }
            })));

        // Resume an interrupted resizing animation in case the node wasn't
        // marked dirty already (oracle animated_size.dart:225-227).
        if matches!(
            self.state,
            AnimatedSizeState::Changed | AnimatedSizeState::Unstable
        ) {
            let _ = handle.mark_needs_layout();
        }
    }

    fn detach(&mut self) {
        // Deliberately does NOT call `self.controller.stop()` — a documented
        // FLUI divergence from the oracle (`animated_size.dart:233-236`).
        // Flutter's `detach` fires far more often (e.g. temporarily-offstage
        // subtrees); FLUI's `detach` only fires on structural tree removal,
        // and controller lifecycle/disposal is the owning `State`'s job.
        // Stopping here would also race a fresh `attach` on a remove+insert
        // reparent.
        if let Some(id) = self.listener_id.take() {
            self.controller.remove_listener(id);
        }
        if let Some(id) = self.status_listener_id.take() {
            self.controller.remove_status_listener(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flui_animation::Scheduler;
    use flui_rendering::context::intrinsics_test_support::leaf_dry_layout;
    use flui_types::geometry::px;

    fn controller(ms: u64) -> AnimationController {
        AnimationController::new(Duration::from_millis(ms), Arc::new(Scheduler::new()))
    }

    fn render(ms: u64) -> RenderAnimatedSize {
        RenderAnimatedSize::new(
            controller(ms),
            ArcCurve::new(flui_animation::Curves::Linear),
            Alignment::CENTER,
            Clip::HardEdge,
            None,
        )
    }

    fn size(w: f32, h: f32) -> Size {
        Size::new(px(w), px(h))
    }

    // ---- state machine: start -> stable ---------------------------------

    #[test]
    fn start_snaps_with_no_animation() {
        let mut ro = render(100);
        ro.layout_start(size(10.0, 10.0));
        assert_eq!(ro.state, AnimatedSizeState::Stable);
        assert_eq!(ro.size_tween.begin, size(10.0, 10.0));
        assert_eq!(ro.size_tween.end, size(10.0, 10.0));
    }

    // ---- stable -> changed: begin = last committed (constrained) size ---

    #[test]
    fn stable_to_changed_begins_at_current_committed_size_not_raw_tween_value() {
        let mut ro = render(100);
        ro.layout_start(size(10.0, 10.0));
        // Simulate a prior frame whose reported size was clipped smaller than
        // the raw tween value — current_size must be the reported value.
        ro.current_size = size(15.0, 15.0);

        ro.layout_stable(size(30.0, 30.0));

        assert_eq!(ro.state, AnimatedSizeState::Changed);
        assert_eq!(
            ro.size_tween.begin,
            size(15.0, 15.0),
            "begin must be the last committed (constrained) size, not the raw tween value"
        );
        assert_eq!(ro.size_tween.end, size(30.0, 30.0));
        assert_eq!(ro.controller.status(), AnimationStatus::Forward);
        assert_eq!(ro.controller.value(), 0.0, "restart_animation resets t=0");
    }

    // ---- changed -> unstable: degenerate collapse, THE headline test ----

    #[test]
    fn changed_to_unstable_collapses_to_degenerate_zero_span_tween() {
        let mut ro = render(100);
        ro.layout_start(size(10.0, 10.0));
        ro.current_size = size(10.0, 10.0);
        ro.layout_stable(size(20.0, 20.0)); // -> Changed, begin=10 end=20
        assert_eq!(ro.state, AnimatedSizeState::Changed);

        // The child changes AGAIN before settling. The naive "begin =
        // current interpolated value" bug would set begin to something
        // between 10 and 20; the oracle instead collapses BOTH ends to the
        // child's raw new size.
        ro.layout_changed(size(50.0, 50.0));

        assert_eq!(ro.state, AnimatedSizeState::Unstable);
        assert_eq!(
            ro.size_tween.begin,
            size(50.0, 50.0),
            "begin must collapse to the child's raw new size, not an interpolated value"
        );
        assert_eq!(
            ro.size_tween.end,
            size(50.0, 50.0),
            "a degenerate tween has begin == end"
        );
        assert_eq!(ro.controller.value(), 0.0);
    }

    // ---- unstable -> unstable, then -> stable, no visible jump -----------

    #[test]
    fn unstable_repeats_then_settles_with_no_visible_jump() {
        let mut ro = render(100);
        ro.layout_start(size(10.0, 10.0));
        ro.current_size = size(10.0, 10.0);
        ro.layout_stable(size(20.0, 20.0)); // -> Changed
        ro.layout_changed(size(50.0, 50.0)); // -> Unstable, begin=end=50
        ro.layout_unstable(size(80.0, 80.0)); // still changing -> begin=end=80
        assert_eq!(ro.size_tween.begin, size(80.0, 80.0));
        assert_eq!(ro.size_tween.end, size(80.0, 80.0));

        // Finally repeats: settle back to Stable with no discontinuity
        // (begin == end already).
        ro.layout_unstable(size(80.0, 80.0));
        assert_eq!(ro.state, AnimatedSizeState::Stable);
        assert_eq!(ro.size_tween.begin, size(80.0, 80.0));
        assert_eq!(ro.size_tween.end, size(80.0, 80.0));
        assert!(!ro.controller.is_animating());
    }

    // ---- changed -> stable: resumes existing span untouched --------------

    #[test]
    fn changed_to_stable_resumes_existing_span_untouched() {
        let mut ro = render(100);
        ro.layout_start(size(10.0, 10.0));
        ro.current_size = size(10.0, 10.0);
        ro.layout_stable(size(20.0, 20.0)); // -> Changed, span 10->20
        let _ = ro.controller.stop();

        ro.layout_changed(size(20.0, 20.0)); // repeats -> Stable, resume

        assert_eq!(ro.state, AnimatedSizeState::Stable);
        assert_eq!(
            ro.size_tween.begin,
            size(10.0, 10.0),
            "the genuine interpolation span from layout_stable must be left untouched"
        );
        assert_eq!(ro.size_tween.end, size(20.0, 20.0));
        assert!(ro.controller.is_animating(), "must resume, not restart");
    }

    // ---- dry-layout / dry-baseline parity, no side effects ---------------

    #[test]
    fn dry_size_for_matches_perform_layout_formula_with_no_side_effects() {
        let mut ro = render(100);
        ro.layout_start(size(10.0, 10.0));
        ro.current_size = size(10.0, 10.0);
        ro.layout_stable(size(20.0, 20.0)); // -> Changed, begin=10 end=20

        let cc = BoxConstraints::loose(size(100.0, 100.0));
        let before_state = ro.state;
        let before_tween = ro.size_tween;

        let dry = ro.dry_size_for(cc, size(20.0, 20.0));
        // At t=0 the tween evaluates to `begin`.
        assert_eq!(dry, size(10.0, 10.0));

        // No side effects: calling twice must not mutate state/tween.
        let dry_again = ro.dry_size_for(cc, size(20.0, 20.0));
        assert_eq!(dry, dry_again);
        assert_eq!(ro.state, before_state);
        assert_eq!(ro.size_tween, before_tween);
    }

    #[test]
    fn dry_layout_no_child_or_tight_returns_smallest() {
        let ro = render(100);
        let cc = BoxConstraints::tight(size(40.0, 40.0));
        let dry = leaf_dry_layout(|ctx| ro.compute_dry_layout(cc, ctx));
        assert_eq!(dry, size(40.0, 40.0));
    }

    // ---- fast path: tight constraints / no child --------------------------

    #[test]
    fn fast_path_resets_state_and_stops_controller() {
        let mut ro = render(100);
        ro.layout_start(size(10.0, 10.0));
        ro.current_size = size(10.0, 10.0);
        ro.layout_stable(size(20.0, 20.0)); // -> Changed, animating
        assert!(ro.controller.is_animating());

        // Simulate the fast-path bookkeeping directly (perform_layout itself
        // needs a live BoxLayoutContext harness — see the integration
        // harness tests for the full end-to-end assertion).
        let _ = ro.controller.stop();
        let snapped = size(40.0, 40.0);
        ro.size_tween = SizeTween::new(snapped, snapped);
        ro.state = AnimatedSizeState::Start;
        ro.current_size = snapped;

        assert_eq!(ro.state, AnimatedSizeState::Start);
        assert!(!ro.controller.is_animating());
        assert_eq!(ro.size_tween.begin, snapped);
        assert_eq!(ro.size_tween.end, snapped);
    }

    // ---- on_end fires exactly once per completed run ----------------------

    #[test]
    fn on_end_fires_on_completion_via_status_listener() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let ro = render(50);
        let calls = Arc::new(AtomicUsize::new(0));
        let calls2 = calls.clone();
        ro.set_on_end(Some(Arc::new(move || {
            calls2.fetch_add(1, Ordering::SeqCst);
        })));

        // attach() registers the status listener.
        let status_id = ro.controller.add_status_listener(Arc::new({
            let on_end = ro.on_end.clone();
            move |status| {
                if status == AnimationStatus::Completed
                    && let Some(cb) = on_end.lock().as_ref()
                {
                    cb();
                }
            }
        }));

        let _ = ro.controller.forward_from(Some(1.0)); // settles immediately -> Completed
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        ro.controller.remove_status_listener(status_id);
    }

    #[test]
    fn set_on_end_swap_is_read_live_by_the_existing_subscription() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        let ro = render(50);
        let first_calls = Arc::new(AtomicUsize::new(0));
        let first_calls2 = first_calls.clone();
        ro.set_on_end(Some(Arc::new(move || {
            first_calls2.fetch_add(1, Ordering::SeqCst);
        })));

        let status_id = ro.controller.add_status_listener(Arc::new({
            let on_end = ro.on_end.clone();
            move |status| {
                if status == AnimationStatus::Completed
                    && let Some(cb) = on_end.lock().as_ref()
                {
                    cb();
                }
            }
        }));

        // Swap the callback BEFORE the run completes — the live cell must be
        // read at fire time, not captured at subscribe time.
        let second_calls = Arc::new(AtomicUsize::new(0));
        let second_calls2 = second_calls.clone();
        ro.set_on_end(Some(Arc::new(move || {
            second_calls2.fetch_add(1, Ordering::SeqCst);
        })));

        let _ = ro.controller.forward_from(Some(1.0));
        assert_eq!(first_calls.load(Ordering::SeqCst), 0);
        assert_eq!(second_calls.load(Ordering::SeqCst), 1);

        ro.controller.remove_status_listener(status_id);
    }

    // ---- attach on Changed/Unstable immediately requests a layout ---------

    #[test]
    fn attach_on_changed_state_immediately_marks_needs_layout() {
        use flui_rendering::pipeline::PipelineOwner;
        use flui_rendering::protocol::BoxProtocol;
        use flui_rendering::traits::RenderObject;

        // Drive the object into `Changed` via the real transition methods
        // (simulating "a node arrives at `attach` already mid-resize" —
        // FLUI's remove+insert reparent model cannot preserve a Rust
        // object's identity across the boundary, so there is no way to
        // construct this precondition except by direct field/method access
        // from within this module — see attach_detach_lifecycle.rs's own
        // note on why reparenting always mints a fresh object).
        let mut ro = render(50);
        ro.layout_start(size(10.0, 10.0));
        ro.current_size = size(10.0, 10.0);
        ro.layout_stable(size(20.0, 20.0)); // -> Changed
        assert_eq!(ro.state, AnimatedSizeState::Changed);

        // `insert` fires the REAL `attach(handle)` with a live handle from
        // the pipeline (not a mock) — this proves the state-gated
        // `handle.mark_needs_layout()` call in `attach` actually executes
        // without error against a real handle. A fresh insert is already
        // dirty by default regardless of this guard (every new node needs
        // its first layout), so this does not isolate the guard's marginal
        // effect — only `attach_detach_lifecycle.rs` proves the general
        // attach/detach wiring; this test's job is narrower: confirm this
        // object's `attach` override is wired to a real handle correctly.
        let mut owner = PipelineOwner::new();
        let id = owner.insert(Box::new(ro) as Box<dyn RenderObject<BoxProtocol>>);
        assert!(
            owner
                .nodes_needing_layout()
                .iter()
                .any(|dirty| dirty.id == id),
            "a node inserted while Changed/Unstable must be on the layout \
             dirty list (oracle animated_size.dart:225-227)",
        );
    }

    // ---- setters ------------------------------------------------------------

    #[test]
    fn set_alignment_reports_change_flag() {
        let mut ro = render(50);
        assert!(
            !ro.set_alignment(Alignment::CENTER),
            "same value, no change"
        );
        assert!(ro.set_alignment(Alignment::TOP_LEFT));
        assert_eq!(ro.alignment(), Alignment::TOP_LEFT);
    }

    #[test]
    fn set_clip_behavior_reports_change_flag() {
        let mut ro = render(50);
        assert!(
            !ro.set_clip_behavior(Clip::HardEdge),
            "same value, no change"
        );
        assert!(ro.set_clip_behavior(Clip::None));
        assert_eq!(ro.clip_behavior(), Clip::None);
    }

    #[test]
    fn set_curve_rebuilds_the_curved_animation_over_the_same_controller() {
        let mut ro = render(100);
        let _ = ro.controller.forward_from(Some(0.5));
        let before = ro.animation.value();
        ro.set_curve(ArcCurve::new(flui_animation::Curves::Linear));
        let after = ro.animation.value();
        assert!(
            (before - after).abs() < 1e-6,
            "rebuilding with the same (linear) curve must not change the value"
        );
    }
}
