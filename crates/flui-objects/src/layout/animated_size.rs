//! `RenderAnimatedSize` — animates its own size toward its child's natural
//! size whenever that size changes, clipping overflow while the animation is
//! in flight.
//!
//! Unlike every other render object in this crate, this one drives its **own**
//! layout over time. The owning `AnimatedSize` view holds a [`SizeMotion`]
//! (the shown size as an [`AnimatedValue`], bound to a `Vsync` by the view,
//! see `docs/adr/ADR-0013-render-object-attach-self-dirty-handle.md`) and
//! hands this object a [`SizeMotionHandle`]. The object subscribes to it in
//! [`attach`](RenderBox::attach), so a tick re-marks this node dirty on its
//! own, decoupled from any widget rebuild, and retargets it from
//! [`perform_layout`](RenderBox::perform_layout) when the child's size
//! changes.
//!
//! The shown size itself is the animated value, not a progress through a
//! tween: a retarget inherits the size's velocity, a spring may overshoot,
//! and reduced motion lands on the exact target size.
//!
//! # The retarget state machine
//!
//! [`AnimatedSizeState`] tracks four states:
//!
//! - [`Start`](AnimatedSizeState::Start) — no committed size yet.
//! - [`Stable`](AnimatedSizeState::Stable) — settled: either idle at the
//!   target size, or finishing a run already in flight.
//! - [`Changed`](AnimatedSizeState::Changed) — the child's size changed once
//!   since the object was `Stable`.
//! - [`Unstable`](AnimatedSizeState::Unstable) — the child's size is changing
//!   every layout; tracked directly (no motion) until it repeats.
//!
//! **The one subtlety that is easy to get wrong**: only the `Stable ->
//! Changed` transition ([`layout_stable`](RenderAnimatedSize::layout_stable))
//! moves the shown size toward the new child size. Every later change while
//! already `Changed`/`Unstable` ([`layout_changed`](RenderAnimatedSize::layout_changed),
//! [`layout_unstable`](RenderAnimatedSize::layout_unstable)) snaps the shown
//! size to the child's raw current size — direct tracking, not motion.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use flui_foundation::Single;
use flui_foundation::geometry::{Point, Rect, Size};
use flui_painting::Alignment;
use flui_painting::paint::Clip;

use crate::subscription::RenderSubscription;
use flui_animation::{
    AnimatedValue, AnimatedValueView, Animation, AnimationRunFuture, MotionSpec, Vsync,
    VsyncRegistrationError,
};
use flui_foundation::panic::PanicRecovery;

use crate::layout::shifted_box::AligningShiftedBox;
use flui_rendering::{
    constraints::{BoxConstraints, Constraints},
    context::{
        BoxDryBaselineCtx, BoxDryLayoutCtx, BoxHitTestContext, BoxIntrinsicsCtx, BoxLayoutContext,
        PaintCx,
    },
    parent_data::BoxParentData,
    pipeline::LocalRenderInvalidationHandle,
    traits::{RenderBox, TextBaseline},
};

/// The shown size of an animated-size box, owned by the view that creates
/// the box.
///
/// The owner binds it to a frame registry, changes its motion, and disposes
/// it; the render object moves it through a [`SizeMotionHandle`]. Dropping
/// the owner cancels the run and releases its registration.
pub struct SizeMotion {
    shared: Rc<SharedSize>,
}

/// A non-owning handle to a [`SizeMotion`], held by [`RenderAnimatedSize`].
///
/// It reads the shown size and retargets it from layout. After its owner is
/// gone, it keeps the last shown size and retargets nothing.
#[derive(Clone)]
pub struct SizeMotionHandle {
    shared: Weak<SharedSize>,
    view: AnimatedValueView<Size>,
}

/// A registry move asked for while the owner was out: to `vsync`, or to no
/// registry at all.
struct DeferredRebind {
    vsync: Option<Vsync>,
}

/// The owner slot is taken out for each call into the animated value, so no
/// `RefCell` borrow is held while it notifies listeners. A call that arrives
/// while the slot is out (from one of those listeners) finds it empty and is
/// refused; a disposal arriving then is applied when the slot returns.
struct SharedSize {
    owner: RefCell<Option<AnimatedValue<Size>>>,
    /// An observer of the owner kept here so handing out handles does not
    /// depend on the owner being in its slot.
    view: AnimatedValueView<Size>,
    disposed: Cell<bool>,
    on_complete: RefCell<Option<Rc<dyn Fn()>>>,
    /// The motion the value runs by.
    motion: RefCell<MotionSpec>,
    /// The motion the owner last asked for. While it differs from `motion`
    /// it is pending: layout admits it with the resize it starts, if any, so
    /// one rebuild that changes both the child size and the motion starts one
    /// run, not two. Keeping the request apart from the applied motion means
    /// a request made while an admission publishes is never overwritten by
    /// that admission's commit.
    requested: RefCell<MotionSpec>,
    /// A registry move asked for while the owner was out, applied when it
    /// returns.
    rebind_to: RefCell<Option<DeferredRebind>>,
}

/// Returns a taken-out owner to its slot when the call ends, including when
/// an authored curve unwinds out of it, so a contained panic does not leave
/// the size without an owner.
struct OwnerOut<'a> {
    shared: &'a SharedSize,
    owner: Option<AnimatedValue<Size>>,
}

impl Drop for OwnerOut<'_> {
    fn drop(&mut self) {
        let Some(mut owner) = self.owner.take() else {
            return;
        };
        // A rebind can reenter through the waker with a disposal or a newer
        // move, so both are rechecked until nothing new arrives.
        let mut refused = Vec::new();
        loop {
            if self.shared.disposed.get() {
                owner.dispose();
                break;
            }
            if std::thread::panicking() {
                break;
            }
            let moved = self.shared.rebind_to.borrow_mut().take();
            let Some(moved) = moved else { break };
            if let Err(error) = owner.rebind(moved.vsync.as_ref()) {
                refused.push(error);
            }
        }
        let displaced = self.shared.owner.borrow_mut().replace(owner);
        debug_assert!(displaced.is_none(), "BUG: the size owner slot was refilled");
        drop(displaced);
        // Diagnostics run once the owner is back: a subscriber that panics
        // must not unwind past its restoration.
        for error in refused {
            tracing::error!(%error, "AnimatedSize lost its frame registry");
        }
    }
}

impl SharedSize {
    fn with_owner<R>(&self, call: impl FnOnce(&mut AnimatedValue<Size>) -> R) -> Option<R> {
        if self.disposed.get() {
            return None;
        }
        let owner = self.owner.borrow_mut().take()?;
        let mut out = OwnerOut {
            shared: self,
            owner: Some(owner),
        };
        Some(call(
            out.owner
                .as_mut()
                .expect("BUG: the owner is out for this call"),
        ))
    }

    /// Whether the owner is gone or disposed, so the shown size no longer
    /// moves toward its target.
    fn is_released(this: &Weak<Self>) -> bool {
        this.upgrade().is_none_or(|shared| shared.disposed.get())
    }

    /// The requested motion, when it is not the one applied.
    fn pending(&self) -> Option<MotionSpec> {
        let requested = self.requested.borrow();
        (*requested != *self.motion.borrow()).then(|| requested.clone())
    }

    /// Records an admitted motion. A newer request stays pending.
    fn commit_motion(&self, motion: MotionSpec) {
        let displaced = std::mem::replace(&mut *self.motion.borrow_mut(), motion);
        drop(displaced);
    }

    /// Reports a completed run to the owner's completion hook. A run
    /// replaced by a retarget resolves canceled and reports nothing.
    fn track(self: &Rc<Self>, run: &AnimationRunFuture) {
        let shared = Rc::downgrade(self);
        run.when_complete_or_cancel(move |outcome| {
            if outcome.is_err() {
                return;
            }
            let hook = shared
                .upgrade()
                .and_then(|shared| shared.on_complete.borrow().clone());
            if let Some(hook) = hook {
                hook();
            }
        });
    }
}

impl SizeMotion {
    /// A resting zero size moved by `motion`, registered with `vsync` when
    /// one is given. Without a registry, admitted motion settles at once.
    pub fn new(motion: MotionSpec, vsync: Option<&Vsync>) -> Self {
        let owner =
            AnimatedValue::with_debug_label("AnimatedSize", Size::ZERO, motion.clone(), vsync)
                .expect("BUG: a zero size is finite");
        let view = owner.animation();
        Self {
            shared: Rc::new(SharedSize {
                owner: RefCell::new(Some(owner)),
                view,
                disposed: Cell::new(false),
                on_complete: RefCell::new(None),
                requested: RefCell::new(motion.clone()),
                motion: RefCell::new(motion),
                rebind_to: RefCell::new(None),
            }),
        }
    }

    /// A handle for the render object that shows this size.
    #[must_use]
    pub fn handle(&self) -> SizeMotionHandle {
        SizeMotionHandle {
            shared: Rc::downgrade(&self.shared),
            view: self.shared.view.clone(),
        }
    }

    /// Asks for later resizes, and the one in flight, to move by `motion`.
    /// The render object admits it at its next layout, together with the
    /// resize that layout starts, if any; a run in flight keeps its shown
    /// size and velocity. Returns whether a motion is now waiting.
    pub fn set_motion(&mut self, motion: MotionSpec) -> bool {
        // The displaced spec drops after the borrow ends: its curve's
        // destructor may reenter this owner.
        let displaced = std::mem::replace(&mut *self.shared.requested.borrow_mut(), motion);
        drop(displaced);
        self.shared.pending().is_some()
    }

    /// Calls `hook` each time a resize run completes; a run replaced by a
    /// retarget does not complete.
    pub fn set_on_complete(&mut self, hook: Option<Rc<dyn Fn()>>) {
        let previous = self.shared.on_complete.replace(hook);
        drop(previous);
    }

    /// Moves the registration to `vsync`, keeping the run in flight.
    ///
    /// # Errors
    /// Returns registration exhaustion after releasing the old seat.
    pub fn rebind(&mut self, vsync: Option<&Vsync>) -> Result<(), VsyncRegistrationError> {
        if let Some(result) = self.shared.with_owner(|owner| owner.rebind(vsync)) {
            return result;
        }
        // The owner is out for a call that is notifying: the move happens
        // when it returns, not never.
        if !self.shared.disposed.get() {
            *self.shared.rebind_to.borrow_mut() = Some(DeferredRebind {
                vsync: vsync.cloned(),
            });
        }
        Ok(())
    }

    /// Cancels the run and releases the registration. Idempotent; the last
    /// shown size stays readable through every handle.
    pub fn dispose(&mut self) {
        // While the slot is out, the call that took it disposes on return.
        let _ = self.shared.with_owner(AnimatedValue::dispose);
        self.shared.disposed.set(true);
    }
}

impl std::fmt::Debug for SizeMotion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SizeMotion")
            .field("motion", &*self.shared.motion.borrow())
            .field("disposed", &self.shared.disposed.get())
            .finish_non_exhaustive()
    }
}

impl SizeMotionHandle {
    fn value(&self) -> Size {
        self.view.value()
    }

    fn is_animating(&self) -> bool {
        self.view.is_animating()
    }

    /// Whether the shown size has stopped short of the target for good: its
    /// owner was dropped or disposed. Settling lands on the target instead.
    fn is_released(&self) -> bool {
        SharedSize::is_released(&self.shared)
    }

    /// Whether the owner asked for a motion that layout has not admitted yet.
    fn has_pending_motion(&self) -> bool {
        self.shared
            .upgrade()
            .is_some_and(|shared| shared.pending().is_some())
    }

    /// Moves the shown size toward `target` from the size and velocity shown,
    /// by the pending motion if the owner asked for one: one admission, so a
    /// run that the motion alone would have settled is never started.
    ///
    /// Returns whether the resize is accounted for: admitted, already the
    /// target, or moot because the owner is gone. A refusal returns `false`
    /// so the caller keeps its state and the next layout asks again.
    fn animate_to(&self, target: Size) -> bool {
        let Some(shared) = self.shared.upgrade() else {
            return true;
        };
        let pending = shared.pending();
        let admitted = shared.with_owner(|owner| {
            (*owner.target() != target).then(|| match &pending {
                Some(motion) => owner.retarget(target, motion.clone()),
                None => owner.animate_to(target),
            })
        });
        match admitted {
            Some(Some(Ok(run))) => {
                if let Some(motion) = pending {
                    shared.commit_motion(motion);
                }
                shared.track(&run);
                true
            }
            Some(Some(Err(error))) => {
                tracing::warn!(%error, "AnimatedSize refused a resize");
                false
            }
            Some(None) => {
                self.apply_pending_motion();
                true
            }
            None => shared.disposed.get(),
        }
    }

    /// Admits the pending motion for a run in flight or for later resizes.
    /// Kept when refused, to be tried again at the next layout.
    fn apply_pending_motion(&self) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        let Some(motion) = shared.pending() else {
            return;
        };
        let admitted = shared.with_owner(|owner| {
            let in_flight = !owner.is_settled();
            owner
                .set_motion(motion.clone())
                .map(|run| in_flight.then_some(run))
        });
        match admitted {
            Some(Ok(run)) => {
                shared.commit_motion(motion);
                if let Some(run) = run {
                    shared.track(&run);
                }
            }
            Some(Err(error)) => tracing::warn!(%error, "AnimatedSize kept its previous motion"),
            None => {}
        }
    }

    /// Shows `size` at once, canceling any run. Already resting there is not
    /// a change: snapping again would notify, re-mark layout and keep
    /// requesting frames for a box that is not moving.
    fn snap_to(&self, size: Size) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        let snapped = shared.with_owner(|owner| {
            if owner.is_settled() && owner.value() == size {
                Ok(())
            } else {
                owner.snap_to(size)
            }
        });
        if let Some(Err(error)) = snapped {
            tracing::warn!(%error, "AnimatedSize refused a size");
        }
    }
}

impl std::fmt::Debug for SizeMotionHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SizeMotionHandle")
            .field("size", &self.value())
            .field("animating", &self.is_animating())
            .finish_non_exhaustive()
    }
}

/// The four-state retarget state machine driving [`RenderAnimatedSize`]'s
/// size-change detection. See the module docs for the precise per-transition
/// rule.
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
/// See the module docs for the retarget state machine. The object never
/// builds or sees a `Vsync`: the owner of its [`SizeMotion`] does.
pub struct RenderAnimatedSize {
    inner: AligningShiftedBox,
    size: SizeMotionHandle,
    /// The size the shown size is moving to (or tracking).
    target: Size,
    state: AnimatedSizeState,
    /// Last frame's already-`constrain()`-ed committed size. Read only by
    /// [`dry_size_for`](Self::dry_size_for)'s `Stable` branch.
    current_size: Size,
    has_visual_overflow: bool,
    clip_behavior: Clip,
    /// Value subscription, cancelled on detach or owner release.
    listener_id: Option<RenderSubscription>,
}

impl RenderAnimatedSize {
    /// Creates a render object that shows the size `size` moves. The owner
    /// of the [`SizeMotion`] behind it chooses the motion and the frame
    /// registry.
    pub fn new(size: SizeMotionHandle, alignment: Alignment, clip_behavior: Clip) -> Self {
        Self {
            inner: AligningShiftedBox::new(alignment),
            size,
            target: Size::ZERO,
            state: AnimatedSizeState::Start,
            current_size: Size::ZERO,
            has_visual_overflow: false,
            clip_behavior,
            listener_id: None,
        }
    }

    /// Updates the alignment and reports layout when changed.
    pub fn set_alignment(&mut self, alignment: Alignment) -> flui_rendering::RenderUpdateImpact {
        if self.inner.set_alignment(alignment) {
            flui_rendering::RenderUpdateImpact::LAYOUT
        } else {
            flui_rendering::RenderUpdateImpact::NONE
        }
    }

    /// Updates the clip behavior and reports paint when changed.
    pub fn set_clip_behavior(&mut self, clip_behavior: Clip) -> flui_rendering::RenderUpdateImpact {
        if self.clip_behavior == clip_behavior {
            return flui_rendering::RenderUpdateImpact::NONE;
        }
        self.clip_behavior = clip_behavior;
        flui_rendering::RenderUpdateImpact::PAINT
    }

    /// Reports layout when the owner has asked for a motion that layout has
    /// not admitted yet.
    pub fn sync_motion(&self) -> flui_rendering::RenderUpdateImpact {
        if self.size.has_pending_motion() {
            flui_rendering::RenderUpdateImpact::LAYOUT
        } else {
            flui_rendering::RenderUpdateImpact::NONE
        }
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

    /// Whether the last layout produced a size smaller than the target
    /// (driving [`paint`](RenderBox::paint) to clip).
    pub fn has_visual_overflow(&self) -> bool {
        self.has_visual_overflow
    }

    // ------------------------------------------------------------------
    // The retarget state machine
    // ------------------------------------------------------------------

    /// `start -> stable`: show the child's size, with no animation on the
    /// very first layout.
    fn layout_start(&mut self, child_size: Size) {
        self.track(child_size);
        self.state = AnimatedSizeState::Stable;
    }

    /// Only reachable when `state == Stable`. The one genuine motion: from
    /// the size shown, with its velocity, toward the new child size.
    fn layout_stable(&mut self, child_size: Size) {
        if self.target != child_size {
            // The target is recorded once the value admits the resize, so an
            // authored curve that panics there leaves the next layout to retry.
            if self.size.animate_to(child_size) {
                self.target = child_size;
                self.state = AnimatedSizeState::Changed;
            }
        }
    }

    /// Only reachable when `state == Changed`. A repeated child size lets
    /// the run started by [`layout_stable`](Self::layout_stable) finish; a
    /// further change switches to direct tracking.
    fn layout_changed(&mut self, child_size: Size) {
        if self.target == child_size {
            self.state = AnimatedSizeState::Stable;
        } else {
            self.track(child_size);
            self.state = AnimatedSizeState::Unstable;
        }
    }

    /// Only reachable when `state == Unstable`. Tracks the child directly
    /// while it keeps changing; once it repeats, the shown size is already
    /// there and the object settles.
    fn layout_unstable(&mut self, child_size: Size) {
        if self.target == child_size {
            self.state = AnimatedSizeState::Stable;
        } else {
            self.track(child_size);
        }
    }

    /// Shows `size` directly, with no motion.
    fn track(&mut self, size: Size) {
        self.target = size;
        self.size.snap_to(size);
    }

    /// Pure per-state sizing rule shared by
    /// [`compute_dry_layout`](RenderBox::compute_dry_layout) and
    /// [`compute_dry_baseline`](RenderBox::compute_dry_baseline) — reads
    /// `state`/`target`/`current_size` as of the last **real** layout
    /// without mutating them (dry layout is a pure query).
    fn dry_size_for(&self, constraints: BoxConstraints, child_size: Size) -> Size {
        if self.size.is_released() {
            // Without an owner the shown size is frozen where it stopped;
            // real layout keeps showing it, whatever the child reports.
            return constraints.constrain(self.size.value());
        }
        match self.state {
            AnimatedSizeState::Start => constraints.constrain(child_size),
            AnimatedSizeState::Stable => {
                if self.target != child_size {
                    constraints.constrain(self.current_size)
                } else if self.size.is_animating() {
                    constraints.constrain(self.size.value())
                } else {
                    constraints.constrain(child_size)
                }
            }
            AnimatedSizeState::Changed | AnimatedSizeState::Unstable => {
                if self.target == child_size {
                    constraints.constrain(self.size.value())
                } else {
                    constraints.constrain(child_size)
                }
            }
        }
    }
}

impl std::fmt::Debug for RenderAnimatedSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderAnimatedSize")
            .field("state", &self.state)
            .field("target", &self.target)
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

    fn perform_layout(
        &mut self,
        ctx: &mut BoxLayoutContext<'_, Single, BoxParentData>,
    ) -> flui_rendering::RenderResult<Size> {
        self.has_visual_overflow = false;
        let constraints = *ctx.constraints();

        // Fast path: no child, or the parent gave us no freedom to animate
        // into. The child is still laid out (so its own subtree stays
        // consistent) but `align_child` is deliberately NOT called — the
        // child keeps whatever offset it had, possibly stale.
        if ctx.child_count() == 0 || constraints.is_tight() {
            if ctx.child_count() > 0 {
                ctx.layout_single_child()?;
            }
            let snapped = constraints.smallest();
            self.track(snapped);
            self.size.apply_pending_motion();
            self.state = AnimatedSizeState::Start;
            self.current_size = snapped;
            return Ok(snapped);
        }

        // Full, un-loosened constraints — matching the parent's own.
        let child_size = ctx.layout_single_child()?;
        match self.state {
            AnimatedSizeState::Start => self.layout_start(child_size),
            AnimatedSizeState::Stable => self.layout_stable(child_size),
            AnimatedSizeState::Changed => self.layout_changed(child_size),
            AnimatedSizeState::Unstable => self.layout_unstable(child_size),
        }
        self.size.apply_pending_motion();

        let size = constraints.constrain(self.size.value());
        self.current_size = size;
        self.inner.align_child(ctx, size, child_size);
        self.inner.record_child_baselines(ctx)?;
        self.has_visual_overflow =
            size.width < self.target.width || size.height < self.target.height;
        Ok(size)
    }

    fn compute_distance_to_actual_baseline(
        &self,
        baseline: TextBaseline,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        Ok(self.inner.actual_baseline(baseline))
    }

    fn compute_min_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        if ctx.child_count() == 0 {
            return Ok(0.0);
        }
        ctx.child_min_intrinsic_width(0, height)
    }

    fn compute_max_intrinsic_width(
        &self,
        height: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        if ctx.child_count() == 0 {
            return Ok(0.0);
        }
        ctx.child_max_intrinsic_width(0, height)
    }

    fn compute_min_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        if ctx.child_count() == 0 {
            return Ok(0.0);
        }
        ctx.child_min_intrinsic_height(0, width)
    }

    fn compute_max_intrinsic_height(
        &self,
        width: f64,
        ctx: &mut BoxIntrinsicsCtx<'_>,
    ) -> flui_rendering::RenderResult<f64> {
        if ctx.child_count() == 0 {
            return Ok(0.0);
        }
        ctx.child_max_intrinsic_height(0, width)
    }

    fn compute_dry_layout(
        &self,
        constraints: BoxConstraints,
        ctx: &mut BoxDryLayoutCtx<'_>,
    ) -> flui_rendering::RenderResult<Size> {
        if ctx.child_count() == 0 || constraints.is_tight() {
            return Ok(constraints.smallest());
        }
        let child_size = ctx.child_dry_layout(0, constraints)?;
        Ok(self.dry_size_for(constraints, child_size))
    }

    fn compute_dry_baseline(
        &self,
        constraints: BoxConstraints,
        baseline: TextBaseline,
        ctx: &mut BoxDryBaselineCtx<'_>,
    ) -> flui_rendering::RenderResult<Option<f64>> {
        if ctx.child_count() == 0 {
            return Ok(None);
        }
        let Some(child_baseline) = ctx.child_dry_baseline(0, constraints, baseline)? else {
            return Ok(None);
        };
        let child_size = ctx.child_dry_layout(0, constraints)?;
        let my_size = if constraints.is_tight() {
            constraints.smallest()
        } else {
            self.dry_size_for(constraints, child_size)
        };
        Ok(Some(self.inner.dry_baseline(
            my_size,
            child_size,
            child_baseline,
        )))
    }

    // Closure is load-bearing: `PaintCx::paint_child` is ambiguous as a method path
    // (Single's zero-arg overload vs the indexed variant on other arities), so the
    // closure cannot be replaced by a method reference.
    #[expect(clippy::redundant_closure_for_method_calls)]
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

    fn attach(&mut self, handle: LocalRenderInvalidationHandle) {
        self.detach();
        let mut recovery = PanicRecovery::new();
        self.listener_id = RenderSubscription::subscribe(
            || Some(Rc::new(self.size.view.clone())),
            Some(&handle),
            flui_rendering::RenderUpdateImpact::LAYOUT,
            &mut recovery,
        );
        recovery.finish();

        // Show a resize that moved while detached, in case the node wasn't
        // marked dirty already.
        if matches!(
            self.state,
            AnimatedSizeState::Changed | AnimatedSizeState::Unstable
        ) {
            let _ = handle.mark_needs_layout();
        }
    }

    fn detach(&mut self) {
        // Deliberately does NOT stop the run: `detach` only fires on
        // structural tree removal (not for temporarily-offstage subtrees),
        // and the run's lifecycle and disposal are the owning view's job.
        // Stopping here would also race a fresh `attach` on a remove+insert
        // reparent.
        if let Some(subscription) = self.listener_id.take() {
            let mut recovery = PanicRecovery::new();
            subscription.close(&mut recovery);
            recovery.finish();
        }
    }
}
