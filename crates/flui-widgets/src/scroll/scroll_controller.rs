//! `ScrollController` — the user-facing handle for reading and driving scroll
//! position. Analogous to Flutter's `ScrollController`.
//!
//! A `ScrollController` is cheaply cloneable: every clone shares the same
//! underlying state via an `Arc`. Gesture callbacks inside `Scrollable` hold
//! a clone and mutate the position; `AnimatedView`'s listenable subscription
//! on the same `Arc` triggers a rebuild whenever the position changes.
//!
//! # Flutter parity
//!
//! Corresponds to `ScrollController` + `ScrollPosition` in
//! `widgets/scroll_controller.dart`. FLUI keeps the split as two types —
//! `ScrollController` (this one) as the user-facing handle, and
//! [`flui_rendering::view::ScrollPosition`] as the shared, `RenderViewport`-
//! consumable state it wraps — but restricts a controller to exactly one
//! position (Flutter's multi-position attach/detach is deferred).
//!
//! # Deferred (v1)
//!
//! - Multiple attached positions (one controller → many scrollables).
//!
//! # `animate_to` (ADR-0037 PR3)
//!
//! [`ScrollController::animate_to`] queues a `PendingScrollCommand` — a
//! private, mutex-guarded cell — rather than driving a ticker itself: the
//! controller has no `AnimationController` of its own to animate with.
//! `Scrollable`'s `ScrollableState` (`scrollable.rs`) already owns one (the
//! vsync-registered fling controller its module docs describe) and services
//! the queued command from its `AnimatedBuilder` rebuild closure, which
//! reruns on every notify this controller fires. A user grab (`on_pan_start`)
//! cancels the run for free, since it `stop()`s that same controller;
//! [`jump_to`](ScrollController::jump_to) cancels through a SECOND, synchronous path — a
//! `stop_hook` installed the same way — because a merely queued cancellation
//! would only take effect on the next rebuild, one frame after
//! `flui-binding`'s `pump_frame` has already ticked the not-yet-cancelled
//! controller (Flutter parity: `ScrollPosition.jumpTo` calls `goIdle()`
//! unconditionally and synchronously, even when the value doesn't change).
//!
//! **Divergence: no `Future`.** The oracle's `ScrollController.animateTo`
//! returns `Future<void>` so a caller can `await` completion
//! (`scroll_controller.dart`, tag `3.44.0`). FLUI has no widget-level async
//! gate to await from `build`/event-handler code, so `animate_to` returns
//! nothing — a caller cannot observe when the run finishes short of polling
//! [`ScrollController::pixels`] or watching [`ScrollController::as_listenable`].
//! Named follow-up: `flui-material`'s `TabController`/`TabBarView`
//! (`tab_controller.rs`'s "`animate_to` is a documented alias" section)
//! documents `TabController::animate_to` as a plain `set_index` alias
//! specifically because no real `AnimationController`/`Ticker` wiring existed
//! anywhere in this crate yet — that rebase is this PR's closure signal, not
//! something this change reaches into `flui-material` to do itself.
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

use std::sync::{Arc, Mutex};
use std::time::Duration;

use flui_animation::{AnimationController, Curve};
use flui_foundation::Listenable;
use flui_rendering::view::{ScrollPosition, ViewportOffset};

/// The synchronous `jump_to` cancellation hook — see [`ScrollController`]'s
/// `stop_hook` field docs.
type StopHook = Arc<dyn Fn() + Send + Sync>;

// ---------------------------------------------------------------------------
// Pending command — the animate_to/jump_to <-> ScrollableState handoff
// ---------------------------------------------------------------------------

/// A command queued by [`ScrollController::animate_to`]/[`jump_to`](ScrollController::jump_to)
/// for [`ScrollController::service_pending_command`] to act on, called from
/// `ScrollableState`'s notify-triggered `AnimatedBuilder` rebuild closure
/// (`scrollable.rs`).
///
/// One slot, not a queue: a later command always supersedes an earlier,
/// not-yet-serviced one — mirrors `ScrollPosition.jumpTo` cancelling whatever
/// activity (ballistic or driven) is currently running, and a second
/// `animateTo` replacing the first
/// (`scroll_position_with_single_context.dart`, tag `3.44.0`).
enum PendingScrollCommand {
    /// Drive the fling controller through a curve/duration tween to
    /// `target_pixels` (already clamped to `[min_scroll_extent,
    /// max_scroll_extent]` by [`ScrollController::animate_to`]).
    AnimateTo {
        target_pixels: f32,
        duration: Duration,
        curve: Arc<dyn Curve + Send + Sync>, // PORT-CHECK-OK-DYN: see PopPacing's doc (navigator/binding.rs) — same erased easing-curve boundary
    },
    /// Stop whatever is currently driving the fling controller — `jump_to`
    /// supersedes any pending or in-flight `animate_to`.
    Cancel,
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
    /// See `PendingScrollCommand`'s docs. Behind a private lock — never
    /// exposed through the public API — and shared across clones via the
    /// same `Arc` every other piece of this controller's state rides on.
    pending_command: Arc<Mutex<Option<PendingScrollCommand>>>,
    /// A synchronous cancellation hook `ScrollableState::init_state` installs
    /// (mirroring `ScrollPosition::set_flush_handle`'s install lifecycle),
    /// closing over the fling `AnimationController` to call `stop()` on it
    /// immediately.
    ///
    /// Needed alongside `PendingScrollCommand::Cancel`, not instead of it:
    /// `flui-binding`'s `pump_frame` ticks vsync-registered controllers
    /// *before* draining the rebuild queue that services a merely-queued
    /// pending command (see its "Ordering" doc). Without this hook, `jump_to`
    /// during an active `animate_to`/fling would queue a `Cancel` that only
    /// takes effect on the NEXT frame's rebuild — one frame too late, since
    /// that same frame's tick step would already have advanced (and
    /// overwritten) the position via the not-yet-stopped controller's value
    /// listener. Calling `stop()` here, synchronously, at `jump_to` call time
    /// closes that gap — matching `ScrollPosition.jumpTo`'s `goIdle()`, which
    /// the oracle also calls synchronously, before touching `pixels`.
    stop_hook: Arc<Mutex<Option<StopHook>>>,
}

impl std::fmt::Debug for ScrollController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollController")
            .field("position", &self.position)
            .field(
                "has_pending_command",
                &self
                    .pending_command
                    .lock()
                    .is_ok_and(|guard| guard.is_some()),
            )
            .field(
                "has_stop_hook",
                &self.stop_hook.lock().is_ok_and(|guard| guard.is_some()),
            )
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
            pending_command: Arc::new(Mutex::new(None)),
            stop_hook: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a new controller pre-seeded at `initial_scroll_offset` pixels,
    /// before any layout has committed extents.
    ///
    /// Flutter parity: `ScrollController(initialScrollOffset: ...)`
    /// (`widgets/scroll_controller.dart`). The value is **not** clamped here —
    /// extents are unknown until the first layout — so it is clamped exactly
    /// like a value set via [`set_pixels`](Self::set_pixels) before mount: the
    /// first `apply_content_dimensions` call a `Scrollable`/`Viewport` in
    /// [`position`](Self::position) mode commits during layout brings it into
    /// `[min_scroll_extent, max_scroll_extent]`.
    #[must_use]
    pub fn with_initial_scroll_offset(initial_scroll_offset: f32) -> Self {
        let controller = Self::new();
        controller.set_pixels(initial_scroll_offset);
        controller
    }

    // -- Reads ---------------------------------------------------------------

    /// Current scroll offset in logical pixels.
    #[must_use]
    pub fn pixels(&self) -> f32 {
        self.position.pixels()
    }

    /// The minimum allowed pixel value (typically 0.0).
    #[must_use]
    pub fn min_scroll_extent(&self) -> f32 {
        self.position.min_scroll_extent()
    }

    /// The maximum allowed pixel value (content length − viewport length).
    #[must_use]
    pub fn max_scroll_extent(&self) -> f32 {
        self.position.max_scroll_extent()
    }

    /// Length of the visible window along the scroll axis in logical pixels.
    /// `0.0` until [`update_dimensions`](Self::update_dimensions) is called
    /// (or, in `position` mode, until the first layout commits it).
    #[must_use]
    pub fn viewport_dimension_pixels(&self) -> f32 {
        self.position.viewport_dimension()
    }

    /// Total scrollable range = `max_scroll_extent - min_scroll_extent`.
    #[must_use]
    pub fn scroll_extent(&self) -> f32 {
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
    pub fn set_pixels(&self, pixels: f32) {
        self.position.set_pixels(pixels);
    }

    /// Jump the scroll position to `pixels`, clamped to
    /// `[min_scroll_extent, max_scroll_extent]`.
    ///
    /// Notifies listeners on a real change; does not animate. Use this for
    /// programmatic jumps (e.g. `jump_to(0.0)` to scroll to the top).
    ///
    /// Flutter parity: `ScrollPosition.jumpTo` calls `goIdle()` — cancelling
    /// whatever activity currently owns the position — unconditionally,
    /// before comparing the value (`scroll_position_with_single_context.dart`,
    /// tag `3.44.0`). This cancels the same way, synchronously, via the
    /// installed `stop_hook` (see that field's doc on [`ScrollController`]):
    /// a ballistic fling or an [`animate_to`](Self::animate_to) run currently
    /// in flight on the driving `ScrollableState` is stopped THIS INSTANT
    /// (not merely queued — see that field's doc for why a frame's delay is
    /// observable), and any not-yet-serviced `animate_to` request is dropped
    /// in favor of this jump.
    pub fn jump_to(&self, pixels: f32) {
        let clamped = pixels.clamp(self.min_scroll_extent(), self.max_scroll_extent());
        if let Some(hook) = self
            .stop_hook
            .lock()
            .expect("BUG: stop_hook mutex poisoned — a panic escaped a locked section")
            .as_ref()
        {
            hook();
        }
        self.set_pending_command(PendingScrollCommand::Cancel);
        self.position.set_pixels(clamped);
    }

    /// Animate the scroll offset from its current value to `target_pixels`
    /// over `duration`, easing through `curve` — clamped to
    /// `[min_scroll_extent, max_scroll_extent]`, same as [`jump_to`](Self::jump_to).
    ///
    /// # Flutter parity
    ///
    /// Mirrors `ScrollController.animateTo` /
    /// `ScrollPositionWithSingleContext.animateTo`
    /// (`scroll_controller.dart`/`scroll_position_with_single_context.dart`,
    /// tag `3.44.0`): any activity currently driving the position — a
    /// ballistic fling, or an earlier `animate_to` — is interrupted, and the
    /// new run starts from wherever the position currently sits. A user grab
    /// (`Scrollable`'s `on_pan_start`) cancels the run for free, since it
    /// stops the very `AnimationController` this drives — see this module's
    /// docs and `scrollable.rs`.
    ///
    /// `duration == Duration::ZERO` jumps immediately via
    /// [`jump_to`](Self::jump_to) instead of scheduling a zero-length
    /// animation — the oracle instead asserts `duration > Duration.zero` at
    /// the driving activity and requires callers to use `jumpTo` for that
    /// case; panicking on a public entry point is not this crate's contract
    /// (`docs/PANIC-POLICY.md`), so this documents the same "duration must be
    /// positive to actually animate" rule as a graceful fallback instead.
    ///
    /// See this module's docs for why this returns nothing where the oracle
    /// returns `Future<void>`.
    pub fn animate_to(
        &self,
        target_pixels: f32,
        duration: Duration,
        curve: Arc<dyn Curve + Send + Sync>, // PORT-CHECK-OK-DYN: see PopPacing's doc (navigator/binding.rs) — same erased easing-curve boundary
    ) {
        if duration.is_zero() {
            self.jump_to(target_pixels);
            return;
        }
        let target = target_pixels.clamp(self.min_scroll_extent(), self.max_scroll_extent());
        self.set_pending_command(PendingScrollCommand::AnimateTo {
            target_pixels: target,
            duration,
            curve,
        });
        self.position.notify();
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
        viewport_dimension_pixels: f32,
        min_scroll_extent: f32,
        max_scroll_extent: f32,
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
    /// it. Idempotent: safe to call from both `init_state` and
    /// `did_change_dependencies`, mirroring `ScrollPosition::set_flush_handle`'s
    /// own re-install tolerance.
    pub(crate) fn set_stop_hook(&self, hook: StopHook) {
        *self
            .stop_hook
            .lock()
            .expect("BUG: stop_hook mutex poisoned — a panic escaped a locked section") =
            Some(hook);
    }

    /// Overwrites the pending-command slot — a later command always
    /// supersedes an earlier, not-yet-serviced one.
    fn set_pending_command(&self, command: PendingScrollCommand) {
        *self
            .pending_command
            .lock()
            .expect("BUG: pending_command mutex poisoned — a panic escaped a locked section") =
            Some(command);
    }

    /// Takes (and clears) the pending command, if any.
    fn take_pending_command(&self) -> Option<PendingScrollCommand> {
        self.pending_command
            .lock()
            .expect("BUG: pending_command mutex poisoned — a panic escaped a locked section")
            .take()
    }

    /// Services one queued command (if any) against `fling` — the same
    /// `AnimationController` `Scrollable`'s `on_pan_end` drives for ballistic
    /// flings. Called from `ScrollableState`'s notify-triggered
    /// `AnimatedBuilder` rebuild closure (`scrollable.rs`) on every rebuild,
    /// so an `animate_to`/`jump_to` call made between rebuilds is picked up
    /// on the very next one.
    pub(crate) fn service_pending_command(&self, fling: &AnimationController) {
        let Some(command) = self.take_pending_command() else {
            return;
        };
        match command {
            PendingScrollCommand::AnimateTo {
                target_pixels,
                duration,
                curve,
            } => {
                // Sync `fling`'s own value to the true current pixel position
                // first: its value listener only pushes FROM the controller
                // INTO this position, so if pixels moved without ticking it (a
                // `jump_to`, or this being the very first animation `fling`
                // has ever driven), its value is stale and animating from it
                // would visibly jump instead of starting from where the
                // position actually sits.
                fling.set_value(self.pixels());
                let _ = fling.animate_to_curved(target_pixels, Some(duration), curve);
            }
            PendingScrollCommand::Cancel => {
                let _ = fling.stop();
            }
        }
    }

    // -- Listenable bridge ---------------------------------------------------

    /// Return an `Arc<dyn Listenable>` pointing at the same shared position.
    ///
    /// Used by `Scrollable` when implementing `AnimatedView::listenable()` —
    /// the `Arc` upcast avoids an extra allocation and keeps all clones
    /// sharing a single notifier.
    #[must_use]
    pub fn as_listenable(&self) -> Arc<dyn Listenable> {
        self.position.as_listenable()
    }

    // -- Scrollbar helpers ---------------------------------------------------

    /// The fraction of the viewport covered by the scrollbar thumb.
    ///
    /// `thumb_fraction = viewport / (viewport + scroll_extent)` — the standard
    /// proportional-scrollbar formula. Returns `1.0` when there is nothing to
    /// scroll (content fits in the viewport), and `0.0` when the viewport
    /// dimension is unknown (before the first layout).
    #[must_use]
    pub fn thumb_fraction(&self) -> f32 {
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
    /// gives the thumb's pixel offset — the same `_thumbOffset` contract as
    /// Flutter's `ScrollbarPainter` (`widgets/scrollbar.dart`, 3.44.0): flush
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
    pub fn thumb_offset_fraction(&self) -> f32 {
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
    #![allow(clippy::float_cmp)] // unit tests assert exact set-then-read values, not computed floats
    use super::*;

    #[test]
    fn new_controller_starts_at_zero() {
        let controller = ScrollController::new();
        assert_eq!(controller.pixels(), 0.0);
        assert_eq!(controller.min_scroll_extent(), 0.0);
        assert_eq!(controller.max_scroll_extent(), 0.0);
    }

    #[test]
    fn set_pixels_updates_position_and_notifies_listener() {
        let controller = ScrollController::new();
        let notified = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = std::sync::Arc::clone(&notified);
        controller.as_listenable().add_listener(Arc::new(move || {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }));

        controller.set_pixels(120.0);

        assert_eq!(controller.pixels(), 120.0);
        assert!(
            notified.load(std::sync::atomic::Ordering::SeqCst),
            "set_pixels should notify listeners"
        );
    }

    #[test]
    fn same_value_set_pixels_does_not_re_notify() {
        // A no-op `set_pixels` no longer re-notifies, because
        // `ScrollPosition::set_pixels` is epsilon-guarded. This test pins
        // that behavior explicitly so a future regression toward "always
        // notify" is visible here, not just inferred from the absence of a
        // pin.
        let controller = ScrollController::new();
        controller.set_pixels(10.0);

        let notified = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = std::sync::Arc::clone(&notified);
        controller.as_listenable().add_listener(Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));

        controller.set_pixels(10.0); // same value
        assert_eq!(
            notified.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "writing the same pixel value must not notify"
        );

        controller.set_pixels(11.0); // real change
        assert_eq!(
            notified.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "writing a different pixel value must notify"
        );
    }

    /// Regression: `update_dimensions`'s own `apply_viewport_dimension`/
    /// `apply_content_dimensions` calls can mark the shared position dirty
    /// and queue a coalesced post-frame flush (whenever a flush handle is
    /// installed — e.g. this controller is also wired into a `Scrollable`).
    /// Before `flush_now` existed, `update_dimensions` notified via a plain
    /// `notify()` that did not consume that queued flush's dirty state, so
    /// the SAME mutation notified twice: once synchronously here, once again
    /// when the frame completed and the flush ran.
    #[test]
    fn update_dimensions_with_a_flush_handle_installed_notifies_exactly_once() {
        let scheduler = flui_scheduler::Scheduler::new();
        let handle = flui_scheduler::PostFrameHandle::new(&scheduler);

        let controller = ScrollController::new();
        controller.position().set_flush_handle(handle);

        let notified = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = std::sync::Arc::clone(&notified);
        controller.as_listenable().add_listener(Arc::new(move || {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }));

        controller.update_dimensions(300.0, 0.0, 500.0);
        assert_eq!(
            notified.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "update_dimensions must notify synchronously exactly once"
        );

        // The coalesced flush apply_content_dimensions queued is still
        // sitting on the scheduler; running the frame must not add a
        // second notification for the same update_dimensions call.
        scheduler.execute_frame();
        assert_eq!(
            notified.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "the frame completing afterward must not double-notify for the same mutation"
        );
    }

    #[test]
    fn with_initial_scroll_offset_seeds_pixels_before_any_layout() {
        let controller = ScrollController::with_initial_scroll_offset(209.0);
        assert_eq!(
            controller.pixels(),
            209.0,
            "with_initial_scroll_offset must seed pixels immediately, before any \
             update_dimensions/layout call establishes real extents"
        );

        // The seed is unclamped (extents are unknown yet, both 0.0) — the same
        // contract as `set_pixels` before mount; a subsequent `update_dimensions`
        // brings it into range.
        controller.update_dimensions(300.0, 0.0, 150.0);
        assert_eq!(
            controller.pixels(),
            150.0,
            "the deferred clamp must apply once real extents arrive, same as any \
             other pre-layout pixel write"
        );
    }

    #[test]
    fn jump_to_clamps_to_extents() {
        let controller = ScrollController::new();
        controller.update_dimensions(300.0, 0.0, 500.0);

        controller.jump_to(-100.0);
        assert_eq!(controller.pixels(), 0.0, "jump_to clamps below min");

        controller.jump_to(800.0);
        assert_eq!(controller.pixels(), 500.0, "jump_to clamps above max");

        controller.jump_to(250.0);
        assert_eq!(
            controller.pixels(),
            250.0,
            "jump_to accepts in-range values"
        );
    }

    #[test]
    fn update_dimensions_clamps_existing_pixels_to_new_extents() {
        let controller = ScrollController::new();
        controller.set_pixels(600.0);
        // New max is 400 — the current 600 must be clamped.
        controller.update_dimensions(300.0, 0.0, 400.0);
        assert_eq!(
            controller.pixels(),
            400.0,
            "update_dimensions must clamp pixels that fall outside the new max"
        );
    }

    #[test]
    fn thumb_fraction_with_equal_viewport_and_scroll_extent() {
        let controller = ScrollController::new();
        // viewport = 400, scroll_extent = 400, content = 800.
        controller.update_dimensions(400.0, 0.0, 400.0);
        let fraction = controller.thumb_fraction();
        // thumb_fraction = 400 / 800 = 0.5
        assert!(
            (fraction - 0.5).abs() < 0.001,
            "thumb fraction should be 0.5 when viewport equals scroll extent, got {fraction}"
        );
    }

    #[test]
    fn thumb_fraction_is_one_when_content_fits() {
        let controller = ScrollController::new();
        // max_extent = 0 means content fits entirely in the viewport.
        controller.update_dimensions(400.0, 0.0, 0.0);
        assert_eq!(
            controller.thumb_fraction(),
            1.0,
            "thumb fraction should be 1.0 when scroll_extent is zero"
        );
    }

    #[test]
    fn thumb_offset_fraction_at_half_scroll() {
        let controller = ScrollController::new();
        controller.update_dimensions(400.0, 0.0, 400.0);
        controller.set_pixels(200.0); // half-way
        // offset_fraction = (200 - 0) / 400 = 0.5 -- a fraction of the
        // AVAILABLE track (0=top, 1=bottom), independent of thumb_fraction;
        // see this method's doc for why `(1 - thumb_fraction)` must NOT be
        // folded in here.
        let offset = controller.thumb_offset_fraction();
        assert!(
            (offset - 0.5).abs() < 0.001,
            "thumb offset fraction at half-scroll should be 0.5, got {offset}"
        );
    }

    /// `thumb_offset_fraction` must reach exactly `1.0` at `max_scroll_extent`
    /// and exactly `0.0` at `min_scroll_extent` — the full `[0, 1]` range its
    /// own doc promises. A version that folds in `(1 - thumb_fraction)` (the
    /// bug this pins the fix for) would instead top out at
    /// `1 - thumb_fraction`, short of `1.0`.
    #[test]
    fn thumb_offset_fraction_spans_the_full_unit_range_between_the_extents() {
        let controller = ScrollController::new();
        // thumb_fraction = 300/600 = 0.5 -- if the old `* (1 - thumb_fraction)`
        // factor were still applied, this would top out at 0.5, not 1.0.
        controller.update_dimensions(300.0, 0.0, 300.0);

        controller.set_pixels(0.0);
        assert_eq!(controller.thumb_offset_fraction(), 0.0);

        controller.set_pixels(300.0);
        assert_eq!(
            controller.thumb_offset_fraction(),
            1.0,
            "thumb_offset_fraction must reach exactly 1.0 at max_scroll_extent"
        );
    }

    #[test]
    fn clones_share_state() {
        let controller = ScrollController::new();
        let clone = controller.clone();
        controller.set_pixels(77.0);
        assert_eq!(
            clone.pixels(),
            77.0,
            "a clone must observe mutations made through the original"
        );
    }

    // -- animate_to / service_pending_command --------------------------------

    /// Builds an unbounded fling-style `AnimationController` — the same
    /// shape `ScrollableState::create_state` constructs (`scrollable.rs`):
    /// wide-open bounds so a driven value is never clamped by the controller
    /// itself, only by `animate_to`'s own pre-clamp of the target.
    fn fling_stub() -> AnimationController {
        AnimationController::with_bounds(
            Duration::from_millis(1),
            Arc::new(flui_scheduler::Scheduler::new()),
            f32::NEG_INFINITY,
            f32::INFINITY,
        )
        .expect("NEG_INFINITY < INFINITY satisfies the bounds invariant")
    }

    #[test]
    fn animate_to_with_zero_duration_jumps_immediately() {
        let controller = ScrollController::new();
        controller.update_dimensions(300.0, 0.0, 500.0);

        controller.animate_to(
            200.0,
            Duration::ZERO,
            Arc::new(flui_animation::Curves::Linear),
        );

        assert_eq!(
            controller.pixels(),
            200.0,
            "a zero-duration animate_to must jump immediately, like jump_to"
        );
    }

    #[test]
    fn animate_to_clamps_the_target_to_the_current_extents() {
        use flui_animation::Animation;

        let controller = ScrollController::new();
        controller.update_dimensions(300.0, 0.0, 500.0);
        let fling = fling_stub();

        controller.animate_to(
            999.0,
            Duration::from_millis(50),
            Arc::new(flui_animation::Curves::Linear),
        );
        controller.service_pending_command(&fling);

        // Past the run's duration: `tick_time_based` snaps to `target_value`.
        fling.tick_at(1.0);
        assert_eq!(
            fling.value(),
            500.0,
            "animate_to must clamp its target to max_scroll_extent (500.0) before \
             driving the fling controller, not animate past it to the raw 999.0 request"
        );
    }

    #[test]
    fn a_second_animate_to_supersedes_the_first_before_either_is_serviced() {
        use flui_animation::Animation;

        let controller = ScrollController::new();
        controller.update_dimensions(300.0, 0.0, 5000.0);
        let fling = fling_stub();

        controller.animate_to(
            500.0,
            Duration::from_millis(100),
            Arc::new(flui_animation::Curves::Linear),
        );
        controller.animate_to(
            900.0,
            Duration::from_millis(100),
            Arc::new(flui_animation::Curves::Linear),
        );
        // Only one command was ever queued: the second call overwrote the
        // first before `service_pending_command` ever ran.
        controller.service_pending_command(&fling);

        fling.tick_at(1.0);
        assert_eq!(
            fling.value(),
            900.0,
            "a second animate_to, queued before the first was ever serviced, must \
             replace it outright — the fling controller must drive toward the \
             SECOND target (900.0), never the first (500.0)"
        );
    }

    #[test]
    fn jump_to_cancels_a_not_yet_serviced_animate_to() {
        use flui_animation::Animation;

        let controller = ScrollController::new();
        controller.update_dimensions(300.0, 0.0, 500.0);
        let fling = fling_stub();
        fling
            .forward()
            .expect("fling_stub is not disposed, forward() must succeed");
        assert!(
            fling.status().is_running(),
            "sanity: forward() leaves the fling controller running"
        );

        controller.animate_to(
            400.0,
            Duration::from_millis(100),
            Arc::new(flui_animation::Curves::Linear),
        );
        // jump_to must overwrite the queued AnimateTo with Cancel — the next
        // service call must stop the controller, not start the animation.
        controller.jump_to(50.0);
        controller.service_pending_command(&fling);

        assert!(
            !fling.status().is_running(),
            "jump_to must cancel a not-yet-serviced animate_to instead of letting \
             it start on the next service_pending_command call"
        );
    }
}
