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
//! `widgets/scroll_controller.dart`. FLUI merges the two into one struct
//! because the one-position-per-controller restriction holds everywhere in v1
//! (multiple-position support is deferred).
//!
//! # Deferred (v1)
//!
//! - Multiple attached positions (one controller → many scrollables).
//! - `animateTo` (driven animation to a target offset) — `set_pixels` is the
//!   only way to move the position in v1; animated-to requires ticking.
//! - `notifyListeners` on `update_dimensions` — listeners are notified on
//!   every `update_dimensions` call so widgets (e.g. `Scrollbar`) always
//!   reflect the latest extent even before a gesture fires.

use std::sync::{Arc, Mutex};

use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};

// ---------------------------------------------------------------------------
// Inner state (heap-allocated, shared across all clones)
// ---------------------------------------------------------------------------

/// The heap-allocated scroll state shared by all clones of a `ScrollController`.
struct ScrollPositionState {
    /// Current scroll offset in logical pixels. Grows positive toward the
    /// content end (matches Flutter's pixel convention).
    pixels: Mutex<f32>,
    /// The smallest pixel value reachable without overscroll. Typically 0.0.
    min_scroll_extent: Mutex<f32>,
    /// The largest pixel value reachable without overscroll.
    /// `max - min` = the total scrollable range.
    max_scroll_extent: Mutex<f32>,
    /// The length of the visible window along the scroll axis (set from
    /// layout; 0.0 until `update_dimensions` is called).
    viewport_dimension_pixels: Mutex<f32>,
    /// Notifier whose listeners are fired whenever any of the above fields
    /// changes. `AnimatedView` subscribes here so position changes trigger
    /// rebuilds.
    notifier: ChangeNotifier,
}

impl Listenable for ScrollPositionState {
    fn add_listener(&self, listener: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(listener)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
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
    inner: Arc<ScrollPositionState>,
}

impl std::fmt::Debug for ScrollController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrollController")
            .field("pixels", &self.pixels())
            .field("min_scroll_extent", &self.min_scroll_extent())
            .field("max_scroll_extent", &self.max_scroll_extent())
            .field(
                "viewport_dimension_pixels",
                &self.viewport_dimension_pixels(),
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
    /// layout is known (typically done by the enclosing `Scrollable`).
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(ScrollPositionState {
                pixels: Mutex::new(0.0),
                min_scroll_extent: Mutex::new(0.0),
                max_scroll_extent: Mutex::new(0.0),
                viewport_dimension_pixels: Mutex::new(0.0),
                notifier: ChangeNotifier::new(),
            }),
        }
    }

    // -- Reads ---------------------------------------------------------------

    /// Current scroll offset in logical pixels.
    #[must_use]
    pub fn pixels(&self) -> f32 {
        *self
            .inner
            .pixels
            .lock()
            .expect("scroll position mutex poisoned")
    }

    /// The minimum allowed pixel value (typically 0.0).
    #[must_use]
    pub fn min_scroll_extent(&self) -> f32 {
        *self
            .inner
            .min_scroll_extent
            .lock()
            .expect("scroll position mutex poisoned")
    }

    /// The maximum allowed pixel value (content length − viewport length).
    #[must_use]
    pub fn max_scroll_extent(&self) -> f32 {
        *self
            .inner
            .max_scroll_extent
            .lock()
            .expect("scroll position mutex poisoned")
    }

    /// Length of the visible window along the scroll axis in logical pixels.
    /// `0.0` until [`update_dimensions`](Self::update_dimensions) is called.
    #[must_use]
    pub fn viewport_dimension_pixels(&self) -> f32 {
        *self
            .inner
            .viewport_dimension_pixels
            .lock()
            .expect("scroll position mutex poisoned")
    }

    /// Total scrollable range = `max_scroll_extent - min_scroll_extent`.
    #[must_use]
    pub fn scroll_extent(&self) -> f32 {
        self.max_scroll_extent() - self.min_scroll_extent()
    }

    // -- Writes --------------------------------------------------------------

    /// Set the scroll offset to `pixels` and notify all listeners.
    ///
    /// No clamping is applied here; the caller is responsible for running the
    /// value through `ScrollPhysics::apply_boundary_conditions` first when
    /// interactive behaviour is desired. This keeps the controller
    /// physics-agnostic (it is equally useful for programmatic jumps, physics
    /// updates, and animation-driven ticks).
    pub fn set_pixels(&self, pixels: f32) {
        *self
            .inner
            .pixels
            .lock()
            .expect("scroll position mutex poisoned") = pixels;
        self.inner.notifier.notify_listeners();
    }

    /// Jump the scroll position to `pixels`, clamped to
    /// `[min_scroll_extent, max_scroll_extent]`.
    ///
    /// Notifies listeners; does not animate. Use this for programmatic jumps
    /// (e.g. `jump_to(0.0)` to scroll to the top).
    pub fn jump_to(&self, pixels: f32) {
        let clamped = pixels.clamp(self.min_scroll_extent(), self.max_scroll_extent());
        self.set_pixels(clamped);
    }

    /// Update the scroll extents and viewport dimension after layout, then
    /// notify listeners.
    ///
    /// This should be called once the enclosing viewport has determined its
    /// own size and the size of its content. The `Scrollable` widget calls
    /// this during `build` when those values are known; a `Scrollbar`
    /// subscribes as a listener so it can redraw its thumb.
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
        *self
            .inner
            .viewport_dimension_pixels
            .lock()
            .expect("scroll position mutex poisoned") = viewport_dimension_pixels;
        *self
            .inner
            .min_scroll_extent
            .lock()
            .expect("scroll position mutex poisoned") = min_scroll_extent;
        *self
            .inner
            .max_scroll_extent
            .lock()
            .expect("scroll position mutex poisoned") = max_scroll_extent;
        // Keep pixels in range after an extent update.
        let current = self.pixels();
        let clamped = current.clamp(min_scroll_extent, max_scroll_extent);
        if (clamped - current).abs() > f32::EPSILON {
            *self
                .inner
                .pixels
                .lock()
                .expect("scroll position mutex poisoned") = clamped;
        }
        self.inner.notifier.notify_listeners();
    }

    // -- Listenable bridge ---------------------------------------------------

    /// Return an `Arc<dyn Listenable>` pointing at the same inner state.
    ///
    /// Used by `Scrollable` when implementing `AnimatedView::listenable()` —
    /// the `Arc` upcast avoids an extra allocation and keeps all clones
    /// sharing a single notifier.
    #[must_use]
    pub fn as_listenable(&self) -> Arc<dyn Listenable> {
        Arc::clone(&self.inner) as Arc<dyn Listenable>
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
        controller.inner.notifier.add_listener(Arc::new(move || {
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
