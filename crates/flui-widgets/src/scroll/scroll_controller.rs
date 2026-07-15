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
//! - `animateTo` (driven animation to a target offset) — `set_pixels` is the
//!   only way to move the position in v1; animated-to requires ticking.
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

use std::sync::Arc;

use flui_foundation::Listenable;
use flui_rendering::view::{ScrollPosition, ViewportOffset};

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
}

impl std::fmt::Debug for ScrollController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollController")
            .field("position", &self.position)
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
    pub fn jump_to(&self, pixels: f32) {
        let clamped = pixels.clamp(self.min_scroll_extent(), self.max_scroll_extent());
        self.position.set_pixels(clamped);
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

    /// The offset fraction of the scrollbar thumb along the track.
    ///
    /// `thumb_offset_fraction = pixels / scroll_extent * (1 - thumb_fraction)`
    /// — the fraction into `[0, 1]` (where 0 = top, 1 = bottom) at which the
    /// top of the thumb sits.
    #[must_use]
    pub fn thumb_offset_fraction(&self) -> f32 {
        let scroll_extent = self.scroll_extent();
        if scroll_extent <= 0.0 {
            return 0.0;
        }
        let thumb_fraction = self.thumb_fraction();
        ((self.pixels() - self.min_scroll_extent()) / scroll_extent) * (1.0 - thumb_fraction)
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
        // thumb_fraction = 0.5; offset_fraction = (200/400) * (1 - 0.5) = 0.25
        let offset = controller.thumb_offset_fraction();
        assert!(
            (offset - 0.25).abs() < 0.001,
            "thumb offset fraction at half-scroll should be 0.25, got {offset}"
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
}
