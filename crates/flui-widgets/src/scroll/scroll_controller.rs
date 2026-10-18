//! `ScrollController` — the user-facing handle for reading and driving scroll
//! position.
//!
//! A `ScrollController` is cheaply cloneable: every clone shares the same
//! owner-local state via an `Rc`. Gesture callbacks inside `Scrollable` hold
//! a clone and mutate the position; `AnimatedView`'s listenable subscription
//! on the same position observes changes.
//!
//! # Handle and position
//!
//! The split is two types — `ScrollController` (this one) as the user-facing
//! handle, and [`flui_rendering::view::ScrollPosition`] as the shared,
//! `RenderViewport`-consumable state it wraps — and a controller is restricted
//! to exactly one position (multi-position attach/detach is deferred).
//!
//! # Deferred (v1)
//!
//! - Multiple attached positions (one controller → many scrollables).
//!
//! # Programmatic motion
//!
//! [`ScrollController::animate_to`] queues a `PendingScrollCommand` — a
//! private, owner-local cell — rather than driving a ticker itself: the
//! controller has no `AnimationController` of its own to animate with.
//! `Scrollable`'s `ScrollableState` (`scrollable.rs`) already owns one (the
//! vsync-registered fling controller its module docs describe) and services
//! the queued command from a listener on this controller's notify. A user
//! grab (the drag's `on_start`) cancels the run for free, since it `stop()`s that
//! same controller; [`jump_to`](ScrollController::jump_to) cancels through a
//! SECOND path as well — a `stop_hook` installed the same way — which fires
//! even when `jump_to` writes a pixel value equal to the current one and so
//! notifies nothing at all (a jump cancels whatever activity owns the position
//! unconditionally and synchronously, even when the value doesn't change).
//!
//! **Requests during a pass.** A `jump_to` made while a pass of the nested
//! scroll channel runs through the attached scrollable (from a participant,
//! a position listener or a motion hook) takes neither path at once: it is
//! recorded in the same slot as `PendingScrollCommand::JumpTo`, so it and an
//! `animate_to` supersede each other in call order, and the scrollable
//! applies the latest when the outermost pass returns (ADR-0209 §3). A jump
//! still waiting when the scrollable lets go of the controller is written.
//!
//! **No completion future.** There is no widget-level async gate to await
//! from `build`/event-handler code, so `animate_to` returns
//! nothing — a caller cannot observe when the run finishes short of polling
//! [`ScrollController::pixels`] or watching [`ScrollController::listenable`].
//! Replacement commits the incoming command before retiring the outgoing
//! curve outside the cell's borrow. Delivery and retirement share first-failure
//! custody. A jump publishes its pixels before displaced pending curve destruction;
//! a command admitted by that destruction remains newer than the jump.
//!
//! # Content-dimension feedback
//!
//! `update_dimensions` is the explicit, out-of-frame extent write a caller
//! (typically a test, or code running outside the render pipeline) uses to
//! seed extents before anything has laid out. When a `Scrollable`/`Viewport`
//! is wired with [`ScrollController::position`] instead, `RenderViewport`'s
//! own layout writes extents into the *same* shared [`ScrollPosition`]
//! directly — see that type's docs for the coalesced post-frame flush that
//! replaces a synchronous notify from inside layout.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use flui_animation::{Animation, AnimationController, AnimationRunFuture, ArcCurve, MotionSpec};
use flui_foundation::Listenable;
use flui_foundation::panic::PanicRecovery;
use flui_rendering::view::{ScrollPosition, ViewportOffset};

/// The synchronous `jump_to` cancellation hook — see [`ScrollController`]'s
/// `stop_hook` field docs.
pub(crate) type StopHook = Rc<dyn Fn()>;

/// The jump a mounted `Scrollable` routes through its scroll chain as a
/// programmatic step (ADR-0209 §3). Returns `false` when it could not, and
/// the controller writes the position itself.
pub(crate) type JumpHook = Rc<dyn Fn(f64) -> bool>;

/// Whether a pass of the nested scroll channel runs through the attached
/// scrollable now: a programmatic request then waits in the command slot
/// until the pass returns (ADR-0209 §3).
pub(crate) type PassHook = Rc<dyn Fn() -> bool>;

// ---------------------------------------------------------------------------
// Pending command — the animate_to/jump_to <-> ScrollableState handoff
// ---------------------------------------------------------------------------

/// A command queued by [`ScrollController::animate_to`]/[`jump_to`](ScrollController::jump_to)
/// for [`ScrollController::service_pending_command`] to act on, called from
/// `ScrollableState`'s notify-triggered command listener
/// (`scrollable.rs`).
///
/// One slot, not a queue: a later command always supersedes an earlier,
/// not-yet-serviced one: a jump cancels whatever activity (ballistic or driven)
/// is currently running, and a second `animate_to` replaces the first.
pub(super) enum PendingScrollCommand {
    /// Drive the fling controller through a curve/duration tween to
    /// `target_pixels` (already clamped to `[min_scroll_extent,
    /// max_scroll_extent]` by [`ScrollController::animate_to`]).
    AnimateTo {
        target_pixels: f64,
        duration: Duration,
        curve: ArcCurve,
    },
    /// A jump asked while a pass of the attached scrollable's chain ran:
    /// clamped to the extents when it lands.
    JumpTo { target_pixels: f64 },
    /// Stop whatever is currently driving the fling controller — `jump_to`
    /// supersedes any pending or in-flight `animate_to`.
    Cancel,
}

#[derive(Default)]
struct CommandSlot {
    pending: Option<PendingScrollCommand>,
    // A live admission keeps its identity even after the widget takes its
    // command. Pointer identity cannot wrap or alias another live admission.
    authority: Option<Rc<()>>,
}

// ---------------------------------------------------------------------------
// Public handle
// ---------------------------------------------------------------------------

/// A shared, cheaply-cloneable handle to a scroll position.
///
/// All clones point to the same underlying `ScrollPositionState`. Gesture
/// callbacks inside [`Scrollable`](super::Scrollable) hold a clone and call
/// [`set_pixels`](Self::set_pixels); the `AnimatedView` listenable subscription
/// on the controller triggers a rebuild whenever the position changes.
///
/// # Creating a controller
///
/// ```rust,ignore
/// let controller = ScrollController::new();
/// Scrollable::new()
///     .controller(controller.clone())
///     .child(/* … */)
/// ```
///
/// # Reading the position
///
/// ```rust,ignore
/// let offset_pixels = controller.pixels();
/// let scrollable_range = controller.max_scroll_extent() - controller.min_scroll_extent();
/// ```
#[derive(Clone)]
pub struct ScrollController {
    position: ScrollPosition,
    /// The latest unserviced command, shared only within the owning UI lane.
    /// Replacement moves outgoing ownership beyond the cell borrow.
    pending_command: Rc<RefCell<CommandSlot>>,
    /// A synchronous cancellation hook `ScrollableState::init_state` installs
    /// (mirroring `ScrollPosition::set_flush_handle`'s install lifecycle),
    /// closing over the fling `AnimationController` to call `stop()` on it
    /// immediately.
    ///
    /// An equal-pixel jump does not notify position listeners, so its pending
    /// cancellation would otherwise wait for another position change. The
    /// hook stops the live driver synchronously even for that equal-pixel jump.
    stop_hook: Rc<RefCell<Option<StopHook>>>,
    /// Installed with `stop_hook`: moves a jump through the attached
    /// scrollable's nested scroll chain.
    jump_hook: Rc<RefCell<Option<JumpHook>>>,
    /// Installed with `stop_hook`: whether a jump must wait for a pass of
    /// that chain to return.
    pass_hook: Rc<RefCell<Option<PassHook>>>,
}

impl std::fmt::Debug for ScrollController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let has_pending_command = self.pending_command.borrow().pending.is_some();
        let has_stop_hook = self.stop_hook.borrow().is_some();
        let has_jump_hook = self.jump_hook.borrow().is_some();
        let has_pass_hook = self.pass_hook.borrow().is_some();
        f.debug_struct("ScrollController")
            .field("position", &self.position)
            .field("has_pending_command", &has_pending_command)
            .field("has_stop_hook", &has_stop_hook)
            .field("has_jump_hook", &has_jump_hook)
            .field("has_pass_hook", &has_pass_hook)
            .finish()
    }
}

impl Default for ScrollController {
    fn default() -> Self {
        Self::new()
    }
}

impl ScrollController {
    /// Create a new controller at pixel offset 0.0 with no known extents.
    ///
    /// Call [`update_dimensions`](Self::update_dimensions) once the viewport's
    /// layout is known (typically done by the enclosing `Scrollable`), or
    /// wire [`position`](Self::position) into a `Scrollable`/`Viewport` so
    /// `RenderViewport`'s own layout feeds extents back automatically.
    #[must_use]
    pub fn new() -> Self {
        Self {
            position: ScrollPosition::zero(),
            pending_command: Rc::new(RefCell::new(CommandSlot::default())),
            stop_hook: Rc::new(RefCell::new(None)),
            jump_hook: Rc::new(RefCell::new(None)),
            pass_hook: Rc::new(RefCell::new(None)),
        }
    }

    /// Create a new controller pre-seeded at `initial_scroll_offset` pixels,
    /// before any layout has committed extents.
    ///
    /// The value is **not** clamped here —
    /// extents are unknown until the first layout — so it is clamped exactly
    /// like a value set via [`set_pixels`](Self::set_pixels) before mount: the
    /// first `apply_content_dimensions` call a `Scrollable`/`Viewport` in
    /// [`position`](Self::position) mode commits during layout brings it into
    /// `[min_scroll_extent, max_scroll_extent]`.
    #[must_use]
    pub fn with_initial_scroll_offset(initial_scroll_offset: f64) -> Self {
        let controller = Self::new();
        controller.set_pixels(initial_scroll_offset);
        controller
    }

    // -- Reads ---------------------------------------------------------------

    /// Current scroll offset in logical pixels.
    #[must_use]
    pub fn pixels(&self) -> f64 {
        self.position.pixels()
    }

    /// The minimum allowed pixel value (typically 0.0).
    #[must_use]
    pub fn min_scroll_extent(&self) -> f64 {
        self.position.min_scroll_extent()
    }

    /// The maximum allowed pixel value (content length − viewport length).
    #[must_use]
    pub fn max_scroll_extent(&self) -> f64 {
        self.position.max_scroll_extent()
    }

    /// Length of the visible window along the scroll axis in logical pixels.
    /// `0.0` until [`update_dimensions`](Self::update_dimensions) is called
    /// (or, in `position` mode, until the first layout commits it).
    #[must_use]
    pub fn viewport_dimension_pixels(&self) -> f64 {
        self.position.viewport_dimension()
    }

    /// Total scrollable range = `max_scroll_extent - min_scroll_extent`.
    #[must_use]
    pub fn scroll_extent(&self) -> f64 {
        self.max_scroll_extent() - self.min_scroll_extent()
    }

    // -- Writes --------------------------------------------------------------

    /// Set the scroll offset to `pixels` and notify listeners if it actually
    /// changed (epsilon-guarded — a same-value write does not re-notify).
    ///
    /// No clamping is applied here; the caller is responsible for running the
    /// value through `ScrollPhysics::apply_boundary_conditions` first when
    /// interactive behaviour is desired. This keeps the controller
    /// physics-agnostic (it is equally useful for programmatic jumps, physics
    /// updates, and animation-driven ticks).
    pub fn set_pixels(&self, pixels: f64) {
        self.position.set_pixels(pixels);
    }

    /// Jump to `pixels`, clamped to `[min_scroll_extent, max_scroll_extent]`
    /// when the jump lands. Notifies listeners on a real change; does not
    /// animate. A target that is not finite is dropped, with a diagnostic,
    /// and replaces nothing.
    ///
    /// Outside a scroll pass the jump lands before this returns. From inside
    /// one (a nested scroll participant, a position listener, a motion hook
    /// of the attached scrollable's chain) it is recorded and lands when the
    /// outermost pass on that chain returns, before the dispatch that ran it
    /// returns; until then [`pixels`](Self::pixels) reads the old offset. A
    /// later `jump_to` or [`animate_to`](Self::animate_to) replaces a jump
    /// that has not landed yet.
    ///
    /// When it lands it ends the run the scrollable drives (a fling, a
    /// settle, an animation) `Preempted`, synchronously and even when the
    /// offset does not change, never a contact or key still held, and moves
    /// the offset as one `Programmatic` step of a sequence of its own.
    pub fn jump_to(&self, pixels: f64) {
        if !pixels.is_finite() {
            tracing::warn!(
                pixels,
                "a scroll jump to a target that is not finite was dropped"
            );
            return;
        }
        let in_pass = self.pass_hook.borrow().clone();
        if in_pass.is_some_and(|in_pass| in_pass()) {
            let mut recovery = PanicRecovery::new();
            let (_, retired) = self.replace_pending_command(PendingScrollCommand::JumpTo {
                target_pixels: pixels,
            });
            recovery.scope().retire(retired);
            recovery.finish();
            return;
        }
        let clamped = pixels.clamp(self.min_scroll_extent(), self.max_scroll_extent());
        let mut recovery = PanicRecovery::new();
        let (authority, retired) = self.replace_pending_command(PendingScrollCommand::Cancel);
        // Stopping can synchronously reenter through the driver's listeners.
        let hook = self.stop_hook.borrow().clone();
        if let Some(hook) = hook {
            recovery.scope().run(|| hook());
            recovery.scope().retire(hook);
        }
        let current = self
            .pending_command
            .borrow()
            .authority
            .as_ref()
            .is_some_and(|current| Rc::ptr_eq(current, &authority));
        if current {
            let jump = self.jump_hook.borrow().clone();
            match jump {
                Some(jump) => {
                    let position = self.position.clone();
                    recovery.scope().run(|| {
                        if !jump(clamped) {
                            position.set_pixels(clamped);
                        }
                    });
                    recovery.scope().retire(jump);
                }
                None => self
                    .position
                    .set_pixels_with_recovery(clamped, &mut recovery.scope()),
            }
        }
        // Pixel publication precedes pending-curve destruction, so a newer jump
        // or animation cannot be overwritten by this jump's accepted tail.
        recovery.scope().retire(retired);
        recovery.finish();
    }

    /// Animate the scroll offset from its current value to `target_pixels`
    /// over `duration`, easing through `curve` — clamped to
    /// `[min_scroll_extent, max_scroll_extent]`, same as [`jump_to`](Self::jump_to).
    ///
    /// Any activity currently driving the position — a
    /// ballistic fling, or an earlier `animate_to` — is interrupted, and the
    /// new run starts from wherever the position currently sits. A user grab
    /// (`Scrollable`'s drag `on_start`) cancels the run for free, since it
    /// stops the very `AnimationController` this drives — see this module's
    /// docs and `scrollable.rs`.
    ///
    /// `duration == Duration::ZERO` jumps immediately via
    /// [`jump_to`](Self::jump_to) instead of scheduling a zero-length
    /// animation: animating needs a positive duration, and panicking on a public
    /// entry point is not this crate's contract (`docs/PANIC-POLICY.md`), so a
    /// zero duration falls back gracefully.
    ///
    /// # Tolerance short-circuit
    ///
    /// `ScrollController` is deliberately physics-agnostic (see this module's
    /// docs), so there is no physics-derived tolerance for "close enough to
    /// jump instead". Exact equality short-circuits only while at rest,
    /// which skips the queued-command round-trip's one-frame delay for the
    /// common "already there" case, but still queues (and pays that one frame)
    /// for a target merely *close* to, but not exactly at, the current
    /// position.
    ///
    /// See this module's docs for why this returns no completion future.
    pub fn animate_to(&self, target_pixels: f64, duration: Duration, curve: ArcCurve) {
        if duration.is_zero() {
            self.jump_to(target_pixels);
            return;
        }
        let target = target_pixels.clamp(self.min_scroll_extent(), self.max_scroll_extent());
        if target == self.pixels() && !self.position.is_scrolling() {
            // Already resting there: jump (a no-op write, since the value doesn't
            // change) rather than queuing a command whose own servicing
            // would just reach `AnimationController::retarget`'s identical
            // "already at target" fast path one frame later. A moving position
            // instead retains its velocity and brakes through retarget.
            self.jump_to(target);
            return;
        }
        let mut recovery = PanicRecovery::new();
        let (_, retired) = self.replace_pending_command(PendingScrollCommand::AnimateTo {
            target_pixels: target,
            duration,
            curve,
        });
        recovery.scope().retire(retired);
        self.position.notify_with_recovery(&mut recovery.scope());
        recovery.finish();
    }

    /// Update the scroll extents and viewport dimension, then unconditionally
    /// notify listeners exactly once.
    ///
    /// This is the explicit, out-of-frame extent write for callers that
    /// don't route through `RenderViewport`'s own layout (tests, and manual
    /// wiring). When this controller's [`position`](Self::position) is
    /// injected into a `Scrollable`/`Viewport`, layout reports extents
    /// through the same shared position directly — see [`ScrollPosition`]'s
    /// docs for that coalesced-flush path.
    ///
    /// This method's own `apply_viewport_dimension`/`apply_content_dimensions`
    /// calls can themselves mark the position dirty and queue a coalesced
    /// flush (if a flush handle happens to be installed — e.g. this
    /// controller is also wired into a `Scrollable`). Notifying via
    /// [`ScrollPosition::flush_now`] rather than a plain `notify` consumes
    /// that queued flush's dirty state first, so it becomes a no-op instead
    /// of firing a second, redundant notification once the frame completes.
    ///
    /// # Arguments
    ///
    /// * `viewport_dimension_pixels` — viewport length along the scroll axis.
    /// * `min_scroll_extent` — smallest reachable pixel value (typically 0.0).
    /// * `max_scroll_extent` — largest reachable pixel value without
    ///   overscroll (≥ `min_scroll_extent`; equal when the content fits in the
    ///   viewport and scrolling is a no-op).
    pub fn update_dimensions(
        &self,
        viewport_dimension_pixels: f64,
        min_scroll_extent: f64,
        max_scroll_extent: f64,
    ) {
        let mut position = self.position.clone();
        // Both write straight into the shared state (and clamp `pixels` to
        // the new range, same as before); `flush_now` below is what makes
        // this synchronous instead of the coalesced layout flush.
        let _ = position.apply_viewport_dimension(viewport_dimension_pixels);
        let _ = position.apply_content_dimensions(min_scroll_extent, max_scroll_extent);
        self.position.flush_now();
    }

    /// The shared [`ScrollPosition`] backing this controller.
    ///
    /// Inject this into a `Scrollable`/`Viewport`'s `.position(...)` builder
    /// so gestures and `RenderViewport`'s committed content extents observe
    /// (and write) the same state this controller reads.
    #[must_use]
    pub fn position(&self) -> ScrollPosition {
        self.position.clone()
    }

    // -- animate_to servicing (ScrollableState only) --------------------------

    /// Installs (or replaces) the synchronous cancellation hook `jump_to`
    /// calls immediately — see the `stop_hook` field's doc for why this
    /// exists alongside `PendingScrollCommand::Cancel` rather than instead of
    /// it. Idempotent: safe to call from `init_state` AND `did_update_view`
    /// (a controller swap must move the hook onto the NEW controller),
    /// mirroring `ScrollPosition::set_flush_handle`'s own re-install
    /// tolerance.
    pub(crate) fn set_stop_hook(&self, hook: StopHook, jump: JumpHook, in_pass: PassHook) {
        let retired = self.stop_hook.borrow_mut().replace(hook);
        let retired_jump = self.jump_hook.borrow_mut().replace(jump);
        let retired_pass = self.pass_hook.borrow_mut().replace(in_pass);
        drop(retired);
        drop(retired_jump);
        drop(retired_pass);
    }

    /// Detach only the hook this owner installed. A later attachment may
    /// have replaced it on the same shared controller.
    pub(crate) fn clear_stop_hook(&self, owned: &StopHook) -> bool {
        let retired = {
            let mut hook = self.stop_hook.borrow_mut();
            if hook
                .as_ref()
                .is_some_and(|current| Rc::ptr_eq(current, owned))
            {
                hook.take()
            } else {
                None
            }
        };
        let removed = retired.is_some();
        let (retired_jump, retired_pass) = if removed {
            (
                self.jump_hook.borrow_mut().take(),
                self.pass_hook.borrow_mut().take(),
            )
        } else {
            (None, None)
        };
        drop(retired);
        drop(retired_jump);
        drop(retired_pass);
        removed
    }

    /// Overwrites the pending-command slot — a later command always
    /// supersedes an earlier, not-yet-serviced one.
    fn replace_pending_command(
        &self,
        command: PendingScrollCommand,
    ) -> (Rc<()>, Option<PendingScrollCommand>) {
        let authority = Rc::new(());
        let retired = {
            let mut slot = self.pending_command.borrow_mut();
            slot.authority = Some(Rc::clone(&authority));
            slot.pending.replace(command)
        };
        (authority, retired)
    }

    /// Takes (and clears) the pending command, if any.
    pub(super) fn take_pending_command(&self) -> Option<PendingScrollCommand> {
        self.pending_command.borrow_mut().pending.take()
    }

    /// Whether a command waits to be serviced.
    pub(super) fn has_pending_command(&self) -> bool {
        self.pending_command.borrow().pending.is_some()
    }

    /// The attached `Scrollable` lets go of this controller
    /// (`ScrollableState::dispose`, or a swap to another controller): an
    /// `animate_to` or cancel it did not service yet is dropped, so it does
    /// not resurface against a different `ScrollableState` the controller is
    /// attached to later. A jump that waited for a pass is an accepted
    /// target and is written, clamped, with no owner left.
    pub(crate) fn retire_pending_command(&self) {
        let pending = self.take_pending_command();
        if let Some(PendingScrollCommand::JumpTo { target_pixels }) = &pending {
            let target = target_pixels.clamp(self.min_scroll_extent(), self.max_scroll_extent());
            let mut recovery = PanicRecovery::new();
            self.position
                .set_pixels_with_recovery(target, &mut recovery.scope());
            recovery.finish();
        }
    }

    /// Consume already-admitted command work outside the pending-slot guard,
    /// against `fling` — the same `AnimationController` a `Scrollable`'s
    /// ballistic runs drive. The attached scrollable's owner calls this from
    /// its notify-triggered command listener, so a command queued by
    /// `animate_to`/`jump_to` is serviced on the notify that follows it.
    ///
    /// Returns the run an `animate_to` started, while it moves.
    pub(super) fn service_command(
        &self,
        command: PendingScrollCommand,
        fling: &AnimationController,
    ) -> Option<AnimationRunFuture> {
        match command {
            PendingScrollCommand::AnimateTo {
                target_pixels,
                duration,
                curve,
            } => {
                if !target_pixels.is_finite() {
                    return None;
                }
                // External position writes can leave the driver behind. Sync
                // those writes, but keep a current driver's published velocity
                // and frame origin when interrupting its existing motion.
                let pixels = self.pixels();
                if fling.value() != pixels {
                    fling.set_value(pixels);
                }
                // `animate_to` runs inside a driven scroll activity, so
                // the scrolling notifier holds true for the run — while the
                // USER direction stays idle, because nobody's finger is down. The
                // fling controller's status listener (installed by
                // `ScrollableState`) ends the activity when the run settles,
                // exactly as it does for a ballistic fling.
                //
                // Rejected or synchronously settled motion must not raise an
                // activity that has no running driver to end it.
                let run = fling
                    .retarget(target_pixels, &MotionSpec::Curve { duration, curve })
                    .ok()
                    .filter(|_| fling.status().is_directional())?;
                self.position().set_is_scrolling(true);
                Some(run)
            }
            PendingScrollCommand::JumpTo { target_pixels } => {
                // No owner can step it through a chain: stop the driver and
                // write the clamped target.
                let _ = fling.stop();
                let target =
                    target_pixels.clamp(self.min_scroll_extent(), self.max_scroll_extent());
                self.set_pixels(target);
                None
            }
            PendingScrollCommand::Cancel => {
                let _ = fling.stop();
                None
            }
        }
    }

    // -- Listenable bridge ---------------------------------------------------

    /// Return an `Rc<dyn Listenable>` pointing at the same shared position.
    ///
    /// Used by `Scrollable` when implementing `AnimatedView::listenable()` —
    /// the `Rc` upcast avoids an extra allocation and keeps all clones
    /// sharing a single notifier.
    #[must_use]
    pub fn listenable(&self) -> std::rc::Rc<dyn Listenable> {
        self.position.listenable()
    }

    // -- Scrollbar helpers ---------------------------------------------------

    /// The fraction of the viewport covered by the scrollbar thumb.
    ///
    /// `thumb_fraction = viewport / (viewport + scroll_extent)` — the standard
    /// proportional-scrollbar formula. Returns `1.0` when there is nothing to
    /// scroll (content fits in the viewport), and `0.0` when the viewport
    /// dimension is unknown (before the first layout).
    #[must_use]
    pub fn thumb_fraction(&self) -> f64 {
        let viewport = self.viewport_dimension_pixels();
        let content_length = viewport + self.scroll_extent();
        if content_length <= 0.0 {
            return 1.0;
        }
        (viewport / content_length).clamp(0.0, 1.0)
    }

    /// The offset fraction of the scrollbar thumb along the *available*
    /// track (the track length minus the thumb's own extent) — the fraction
    /// into `[0, 1]` (where 0 = top, 1 = bottom) at which the top of the
    /// thumb sits.
    ///
    /// `thumb_offset_fraction = (pixels - min_scroll_extent) / scroll_extent`.
    /// Multiplying this by `available_track` (`viewport_dimension_pixels -
    /// thumb_height`, using the ACTUAL, already-min-clamped thumb height)
    /// gives the thumb's pixel offset: flush
    /// with the track's start at `pixels == min_scroll_extent` and flush with
    /// its end at `pixels == max_scroll_extent`, in both the unclamped and
    /// min-thumb-length-clamped cases.
    ///
    /// This does NOT itself fold in `(1 - thumb_fraction)` — a caller must
    /// not multiply by the raw `viewport_dimension_pixels` (which does, for
    /// an unclamped thumb, equal `available_track / (1 - thumb_fraction)`,
    /// silently double-applying the factor and stopping short of the
    /// track's end).
    #[must_use]
    pub fn thumb_offset_fraction(&self) -> f64 {
        let scroll_extent = self.scroll_extent();
        if scroll_extent <= 0.0 {
            return 0.0;
        }
        (self.pixels() - self.min_scroll_extent()) / scroll_extent
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- animate_to / service_pending_command --------------------------------

    /// Builds an unbounded fling-style `AnimationController` — the same
    /// shape `ScrollableState::create_state` constructs (`scrollable.rs`):
    /// a driven value is never clamped by the controller itself, only by
    /// `animate_to`'s own pre-clamp of the target.
    fn fling_stub() -> AnimationController {
        AnimationController::builder(Duration::from_millis(1))
            .debug_label("ScrollController.animate_to")
            .unbounded()
            .build()
    }

    /// `jump_to` snapshots its cancellation hook before calling it. Stopping
    /// a driver can synchronously invoke a status listener that jumps through
    /// the same handle; that call must find no outstanding controller borrow.
    ///
    /// The risky call runs on a background thread with a bounded
    /// `recv_timeout`: a real deadlock would hang forever, so this turns a
    /// regression into a fast, clean test failure instead of hanging the
    /// whole suite.
    #[test]
    fn jump_to_does_not_deadlock_when_its_stop_hook_reenters_jump_to() {
        use flui_animation::Animation;

        let (tx, rx) = std::sync::mpsc::channel();
        // Construct and drive all UI state on the worker's own thread. Only
        // the watchdog channel crosses threads.
        std::thread::spawn(move || {
            let controller = ScrollController::new();
            controller.update_dimensions(300.0, 0.0, 500.0);
            let fling = fling_stub();

            // Establish a genuine RUNNING state FIRST, before any listener/hook
            // is wired up — otherwise starting the run itself would trigger the
            // reentrant cascade below synchronously, on this (main) thread,
            // before the sanity check even runs. `fling_stub` is unbounded
            // (#1183): `forward()` targets an infinite bound and is refused, so
            // `animate_to` with a large finite target is the unbounded-safe
            // substitute.
            fling
                .animate_to(1_000_000.0, None)
                .expect("fling_stub is not disposed, animate_to() must succeed");
            assert!(
                fling.status().is_directional(),
                "sanity: animate_to() leaves the fling controller running, so the \
             stop() fired from jump_to's hook below is a genuine status \
             transition (not a no-op the listener would never see)"
            );

            // A status listener that re-enters `jump_to` on the SAME controller —
            // fires synchronously from inside `fling.stop()` (called by the
            // installed hook), on whichever thread calls it.
            let reentrant_controller = controller.clone();
            fling
                .subscribe_status(std::rc::Rc::new(move |_status| {
                    reentrant_controller.jump_to(0.0);
                }))
                .detach();

            let hook_target = fling;
            controller.set_stop_hook(
                Rc::new(move || {
                    let _ = hook_target.stop();
                }),
                Rc::new(|_| false),
                Rc::new(|| false),
            );

            controller.jump_to(100.0);
            let _ = tx.send(());
        });

        assert!(
            rx.recv_timeout(Duration::from_secs(2)).is_ok(),
            "jump_to must not deadlock when its stop_hook synchronously \
             re-enters jump_to via a status listener — snapshot the hook \
             and release its storage before invocation"
        );
    }
}
