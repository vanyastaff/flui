//! [`RefreshIndicator`] — pull-to-refresh gesture wrapper.
//!
//! `RefreshIndicator` manages scrolling internally for its content child and
//! detects a downward overscroll at the top. When the pull distance exceeds
//! [`RefreshIndicator::threshold_px`], releasing the pointer fires
//! [`on_refresh`](RefreshIndicator::on_refresh) and shows a visual indicator
//! until the caller calls [`RefreshController::finish`].
//!
//! # Completion model
//!
//! `on_refresh` is a synchronous `Fn()`. The widget transitions to the
//! *refreshing* state on the same call stack as the pan-end event; the caller
//! signals completion by calling [`RefreshController::finish`] on the
//! [`RefreshController`] it provided.
//!
//! ```text
//! on_refresh fires  →  show spinner
//! caller.finish()   →  hide spinner, return to idle
//! ```
//!
//! # Deferred (v1)
//!
//! - DEFERRED (v1): animated rotation spinner — current indicator is a static
//!   `ColoredBox`. A full `RotationTransition`-based spinner requires a
//!   dedicated vsync-registered `AnimationController`.
//! - DEFERRED (v1): pull-distance → indicator progress easing curve.
//! - DEFERRED (v1): overscroll glow effect.
//! - DEFERRED (v1): nested-scroll coordination and horizontal pull-to-refresh.
//! - DEFERRED (v1): custom indicator builder callbacks.
//!
//! # Flutter parity
//!
//! Corresponds to `widgets/refresh_indicator.dart` `RefreshIndicator`. FLUI
//! v1 uses a synchronous `Fn()` completion model rather than Dart's `Future`
//! because the view layer has no async executor.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use flui_animation::{Animation, AnimationController, Scheduler, Vsync, VsyncRegistration};
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_types::Color;
use flui_view::prelude::StatefulView;
use flui_view::{BuildContext, BuildContextExt, Child, IntoView, ViewExt, ViewState};

use crate::animated::VsyncScope;
use crate::scroll::single_child_scroll_view::SingleChildScrollView;
use crate::scroll::{ClampingScrollPhysics, ScrollController, SharedScrollPhysics};
use crate::{AnimatedBuilder, ColoredBox, GestureDetector, Positioned, Stack};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default pull distance (logical pixels) required to trigger a refresh.
/// Matches Flutter's `kRefreshIndicatorTriggerDistance`.
const DEFAULT_THRESHOLD_PX: f32 = 80.0;

/// Height of the indicator overlay while refreshing (logical pixels).
/// Matches Flutter's `kRefreshIndicatorExtent` (56 dp, a standard FAB height).
const INDICATOR_HEIGHT_PX: f32 = 56.0;

/// Indicator background colour: Material Blue 500 at 80 % opacity.
/// DEFERRED (v1): theming / custom indicator builders.
const INDICATOR_COLOR: Color = Color {
    r: 33,
    g: 150,
    b: 243,
    a: 204,
};

// ---------------------------------------------------------------------------
// RefreshControllerInner — Arc-shared state
// ---------------------------------------------------------------------------

struct RefreshControllerInner {
    pull_distance_px: Mutex<f32>,
    is_refreshing: Mutex<bool>,
    notifier: ChangeNotifier,
}

impl std::fmt::Debug for RefreshControllerInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let is_refreshing = self.is_refreshing.lock().is_ok_and(|g| *g);
        let pull = self.pull_distance_px.lock().map_or(0.0, |g| *g);
        f.debug_struct("RefreshControllerInner")
            .field("is_refreshing", &is_refreshing)
            .field("pull_distance_px", &pull)
            .finish_non_exhaustive()
    }
}

impl Listenable for RefreshControllerInner {
    fn add_listener(&self, callback: ListenerCallback) -> ListenerId {
        self.notifier.add_listener(callback)
    }

    fn remove_listener(&self, id: ListenerId) {
        self.notifier.remove_listener(id);
    }

    fn remove_all_listeners(&self) {
        self.notifier.remove_all_listeners();
    }
}

// ---------------------------------------------------------------------------
// RefreshController — public caller handle
// ---------------------------------------------------------------------------

/// Caller-facing handle for querying the refresh phase and signalling
/// completion.
///
/// Create via [`RefreshController::new`], pass to
/// [`RefreshIndicator::controller`], and call [`finish`](Self::finish) after
/// the refresh operation completes to dismiss the spinner.
///
/// Every clone shares the same inner state via `Arc`.
#[derive(Clone, Debug)]
pub struct RefreshController {
    inner: Arc<RefreshControllerInner>,
}

impl Default for RefreshController {
    fn default() -> Self {
        Self {
            inner: Arc::new(RefreshControllerInner {
                pull_distance_px: Mutex::new(0.0),
                is_refreshing: Mutex::new(false),
                notifier: ChangeNotifier::new(),
            }),
        }
    }
}

impl RefreshController {
    /// Create a controller in the idle (non-refreshing) state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` while [`on_refresh`](RefreshIndicator::on_refresh) has
    /// been called but [`finish`](Self::finish) has not yet been called.
    #[must_use]
    pub fn is_refreshing(&self) -> bool {
        *self
            .inner
            .is_refreshing
            .lock()
            .expect("refresh controller mutex poisoned: is_refreshing field corrupted")
    }

    /// The current pull distance in logical pixels.
    ///
    /// Non-zero only while the user is actively overscrolling past the top.
    /// Resets to `0.0` when the pointer lifts or a refresh begins.
    #[must_use]
    pub fn pull_distance_px(&self) -> f32 {
        *self
            .inner
            .pull_distance_px
            .lock()
            .expect("refresh controller mutex poisoned: pull_distance_px field corrupted")
    }

    /// Signal that the refresh operation is complete. Hides the spinner and
    /// transitions back to idle.
    ///
    /// Calling when not refreshing is a no-op (safe to call defensively).
    pub fn finish(&self) {
        let mut guard = self
            .inner
            .is_refreshing
            .lock()
            .expect("refresh controller mutex poisoned: is_refreshing field corrupted");
        *guard = false;
        drop(guard);
        self.inner.notifier.notify_listeners();
    }

    /// An `Arc<dyn Listenable>` pointing at the same inner state.
    ///
    /// Subscribe via [`AnimatedBuilder`] to rebuild when the refresh phase or
    /// pull distance changes.
    #[must_use]
    pub fn as_listenable(&self) -> Arc<dyn Listenable> {
        Arc::clone(&self.inner) as Arc<dyn Listenable>
    }

    // -- crate-internal mutation called from gesture callbacks ----------------

    pub(super) fn set_pull_distance_px(&self, distance_px: f32) {
        *self
            .inner
            .pull_distance_px
            .lock()
            .expect("refresh controller mutex poisoned: pull_distance_px field corrupted") =
            distance_px;
        self.inner.notifier.notify_listeners();
    }

    pub(super) fn begin_refresh(&self) {
        {
            let mut refreshing = self
                .inner
                .is_refreshing
                .lock()
                .expect("refresh controller mutex poisoned: is_refreshing field corrupted");
            let mut pull = self
                .inner
                .pull_distance_px
                .lock()
                .expect("refresh controller mutex poisoned: pull_distance_px field corrupted");
            *refreshing = true;
            *pull = 0.0;
        }
        self.inner.notifier.notify_listeners();
    }
}

// ---------------------------------------------------------------------------
// RefreshIndicator — StatefulView
// ---------------------------------------------------------------------------

/// Pull-to-refresh gesture wrapper that fires a callback when the user drags
/// down past the top of the scrollable content by more than
/// [`threshold_px`](Self::threshold_px) and releases.
///
/// # Content child
///
/// The `child` is the **scrollable content** (e.g. a `SizedBox` or render
/// widget), **not** a [`Scrollable`](super::Scrollable) widget.
/// `RefreshIndicator` manages the scroll gesture internally to prevent
/// arena competition with a nested `Scrollable`.
///
/// # Example
///
/// ```rust,ignore
/// let refresh_ctrl = RefreshController::new();
/// let scroll_ctrl  = ScrollController::new();
/// scroll_ctrl.update_dimensions(400.0, 0.0, 1600.0);
///
/// RefreshIndicator::new()
///     .controller(refresh_ctrl.clone())
///     .scroll_controller(scroll_ctrl)
///     .on_refresh(|| { /* start background work */ })
///     .child(MyContent::new())
/// // later, after the work is done: refresh_ctrl.finish()
/// ```
#[derive(Clone, StatefulView)]
pub struct RefreshIndicator {
    /// Scrollable content — NOT a `Scrollable` widget.
    child: Child,
    /// Caller handle for querying state and calling `finish()`.
    controller: RefreshController,
    /// Fired when the user releases after an over-threshold pull.
    on_refresh: Arc<dyn Fn() + Send + Sync>,
    /// Minimum overscroll distance (logical pixels) to trigger refresh.
    threshold_px: f32,
    /// Scroll boundary / fling behaviour.
    physics: SharedScrollPhysics,
    /// Scroll position shared between gesture callbacks and the view tree.
    /// Call [`ScrollController::update_dimensions`] before layout.
    scroll_controller: ScrollController,
}

impl std::fmt::Debug for RefreshIndicator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshIndicator")
            .field("threshold_px", &self.threshold_px)
            .field("controller", &self.controller)
            .field("scroll_controller", &self.scroll_controller)
            .finish_non_exhaustive()
    }
}

impl Default for RefreshIndicator {
    fn default() -> Self {
        Self {
            child: Child::empty(),
            controller: RefreshController::new(),
            on_refresh: Arc::new(|| {}),
            threshold_px: DEFAULT_THRESHOLD_PX,
            physics: Arc::new(ClampingScrollPhysics::default()),
            scroll_controller: ScrollController::new(),
        }
    }
}

impl RefreshIndicator {
    /// Create a `RefreshIndicator` with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the scrollable content child.
    ///
    /// Provide the **content** widget, not a `Scrollable`.
    #[must_use]
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.child = Child::some(child.into_view());
        self
    }

    /// Attach the [`RefreshController`] the caller uses to call
    /// [`finish`](RefreshController::finish).
    #[must_use]
    pub fn controller(mut self, controller: RefreshController) -> Self {
        self.controller = controller;
        self
    }

    /// Attach a [`ScrollController`] to share the scroll position externally.
    ///
    /// Call [`ScrollController::update_dimensions`] before layout so physics
    /// boundaries are correct on the first frame.
    #[must_use]
    pub fn scroll_controller(mut self, sc: ScrollController) -> Self {
        self.scroll_controller = sc;
        self
    }

    /// Set the callback fired when a sufficient pull completes.
    ///
    /// Called synchronously on the frame the pointer lifts. Signal completion
    /// by calling [`RefreshController::finish`] on the controller provided to
    /// [`controller`](Self::controller).
    #[must_use]
    pub fn on_refresh(mut self, f: impl Fn() + Send + Sync + 'static) -> Self {
        self.on_refresh = Arc::new(f);
        self
    }

    /// Override the pull threshold in logical pixels (default: `80.0`).
    #[must_use]
    pub fn threshold_px(mut self, threshold: f32) -> Self {
        self.threshold_px = threshold;
        self
    }

    /// Override scroll physics (default: [`ClampingScrollPhysics`]).
    #[must_use]
    pub fn physics(mut self, physics: SharedScrollPhysics) -> Self {
        self.physics = physics;
        self
    }
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Persistent state for [`RefreshIndicator`].
///
/// Owns the fling [`AnimationController`] and its vsync registration,
/// mirroring the pattern used by [`Scrollable`](super::Scrollable).
pub struct RefreshIndicatorState {
    /// The scroll controller from the current view configuration.
    /// Updated in `did_update_view` when the caller swaps controllers.
    scroll_controller: ScrollController,
    /// Ballistic simulation driver (wide-open bounds so pixel values are never
    /// clamped by the controller itself). A value listener pushes current pixel
    /// values into `scroll_controller` each vsync tick.
    fling_controller: AnimationController,
    /// Listener ID on `fling_controller`; removed in `dispose`.
    fling_listener_id: Option<ListenerId>,
    /// Vsync handle kept for `unregister` in `dispose`.
    vsync: Option<Vsync>,
    /// Registration returned by `vsync.register(fling_controller)`.
    vsync_registration: Option<VsyncRegistration>,
}

impl std::fmt::Debug for RefreshIndicatorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshIndicatorState")
            .field("scroll_controller", &self.scroll_controller)
            .field("fling_registered", &self.vsync_registration.is_some())
            .finish_non_exhaustive()
    }
}

impl StatefulView for RefreshIndicator {
    type State = RefreshIndicatorState;

    fn create_state(&self) -> Self::State {
        // Wide-open bounds: pixel values from the ballistic simulation are
        // never clamped by the controller — the simulation's own `is_done`
        // terminates the run. NEG_INFINITY < INFINITY satisfies the bounds check.
        let fling_controller = AnimationController::with_bounds(
            Duration::from_millis(1),
            Arc::new(Scheduler::new()),
            f32::NEG_INFINITY,
            f32::INFINITY,
        )
        .expect("NEG_INFINITY < INFINITY satisfies the bounds invariant");

        RefreshIndicatorState {
            scroll_controller: self.scroll_controller.clone(),
            fling_controller,
            fling_listener_id: None,
            vsync: None,
            vsync_registration: None,
        }
    }
}

impl ViewState<RefreshIndicator> for RefreshIndicatorState {
    fn init_state(&mut self, ctx: &dyn BuildContext) {
        // Push the fling simulation's current pixel value into the scroll
        // controller on each tick — the same wiring Scrollable uses.
        let fling_ref = self.fling_controller.clone();
        let scroll_ref = self.scroll_controller.clone();
        let listener_id = self.fling_controller.add_listener(Arc::new(move || {
            scroll_ref.set_pixels(fling_ref.value());
        }));
        self.fling_listener_id = Some(listener_id);

        // Register with the ambient VsyncScope so the binding ticks the fling
        // controller on each virtual frame deterministically.
        if let Some(vsync) = ctx.get::<VsyncScope, _>(|scope| scope.vsync().clone()) {
            let registration = vsync.register(self.fling_controller.clone());
            self.vsync = Some(vsync);
            self.vsync_registration = Some(registration);
        }
        // Without a VsyncScope the fling controller falls back to wall-clock
        // scheduling — still functional on a real display, not deterministic in tests.
    }

    fn build(&self, view: &RefreshIndicator, _ctx: &dyn BuildContext) -> impl IntoView {
        let scroll_controller = self.scroll_controller.clone();
        let fling_controller = self.fling_controller.clone();
        let refresh_controller = view.controller.clone();
        let on_refresh_fn = view.on_refresh.clone();
        let threshold_px = view.threshold_px;
        let physics = view.physics.clone();
        let child = view.child.clone();

        // Outer AnimatedBuilder: rebuilds on every scroll-position change.
        AnimatedBuilder::new(scroll_controller.as_listenable(), move || {
            let rc_outer = refresh_controller.clone();
            let sc_inner = scroll_controller.clone();
            let fc_inner = fling_controller.clone();
            let ph_inner = physics.clone();
            let ch_inner = child.clone();
            let on_refresh_inner = on_refresh_fn.clone();

            // Inner AnimatedBuilder: rebuilds on refresh-phase / pull-distance
            // changes (fired by RefreshController's own notifier).
            AnimatedBuilder::new(rc_outer.as_listenable(), move || {
                let pixels = sc_inner.pixels();
                let is_refreshing = rc_outer.is_refreshing();

                // Visual indicator: static coloured overlay while refreshing.
                // DEFERRED (v1): animated rotation spinner via RotationTransition.
                let show_indicator = is_refreshing;

                // Gesture clones — each closure needs its own Arc-counted handle.
                let fling_stop = fc_inner.clone();
                let sc_update = sc_inner.clone();
                let rc_update = rc_outer.clone();
                let ph_update = ph_inner.clone();
                let sc_end = sc_inner.clone();
                let rc_end = rc_outer.clone();
                let ph_end = ph_inner.clone();
                let fc_fling = fc_inner.clone();
                let on_refresh_cb = on_refresh_inner.clone();

                let scroll_view = {
                    let mut sv = SingleChildScrollView::new().offset(pixels);
                    if let Some(content) = ch_inner.clone().into_inner() {
                        sv = sv.child(content);
                    }
                    sv
                };

                let mut stack_children: Vec<_> = vec![scroll_view.boxed()];
                if show_indicator {
                    // Overlay the spinner at the very top of the content area.
                    // DEFERRED (v1): replace with RotationTransition-based spinner.
                    let indicator = Positioned::new(ColoredBox::new(INDICATOR_COLOR))
                        .top(0.0)
                        .left(0.0)
                        .right(0.0)
                        .height(INDICATOR_HEIGHT_PX);
                    stack_children.push(indicator.boxed());
                }

                GestureDetector::new()
                    .behavior(HitTestBehavior::Opaque)
                    .on_pan_start(move |_details| {
                        // Halt any in-flight fling when the user grabs the content.
                        let _ = fling_stop.stop();
                    })
                    .on_pan_update(move |details| {
                        // Ignore scroll/pull updates while a refresh is in progress
                        // so the indicator stays stable.
                        if rc_update.is_refreshing() {
                            return;
                        }
                        // Flutter convention: positive dy (finger moving DOWN) maps
                        // to a decrease in scroll offset (reveals content above).
                        let raw_delta_y = details.delta.dy.get();
                        let proposed = sc_update.pixels() - raw_delta_y;

                        if proposed < sc_update.min_scroll_extent() {
                            // Overscroll at top: track how far past the boundary
                            // the user has pulled; freeze the scroll at min_extent.
                            let overscroll_px = sc_update.min_scroll_extent() - proposed;
                            rc_update.set_pull_distance_px(overscroll_px);
                            sc_update.set_pixels(sc_update.min_scroll_extent());
                        } else {
                            rc_update.set_pull_distance_px(0.0);
                            let clamped = ph_update.apply_boundary_conditions(
                                proposed,
                                sc_update.min_scroll_extent(),
                                sc_update.max_scroll_extent(),
                            );
                            sc_update.set_pixels(clamped);
                        }
                    })
                    .on_pan_end(move |details| {
                        let pull = rc_end.pull_distance_px();
                        if pull >= threshold_px {
                            // Sufficient overscroll: enter refreshing state and
                            // fire the caller's callback.
                            rc_end.begin_refresh();
                            on_refresh_cb();
                        } else {
                            // Under-threshold pull: reset and start a normal fling.
                            rc_end.set_pull_distance_px(0.0);

                            // Convert pointer velocity to scroll velocity (negate:
                            // finger DOWN = positive dy → offset increases with negative delta).
                            let fling_vel_px_per_sec = {
                                let raw = -details.velocity.pixels_per_second.dy.get();
                                let bounded = raw.clamp(-8_000.0, 8_000.0);
                                // `clamp` propagates NaN (IEEE 754); treat NaN as 0
                                // so spring-back still works without measurable velocity.
                                if bounded.is_nan() { 0.0 } else { bounded }
                            };
                            if let Some(sim) = ph_end.create_ballistic_simulation(
                                fling_vel_px_per_sec,
                                sc_end.pixels(),
                                sc_end.min_scroll_extent(),
                                sc_end.max_scroll_extent(),
                            ) {
                                let _ = fc_fling.animate_with(sim);
                            }
                        }
                    })
                    .child(Stack::new(stack_children))
            })
        })
    }

    fn did_update_view(&mut self, _old_view: &RefreshIndicator, new_view: &RefreshIndicator) {
        self.scroll_controller = new_view.scroll_controller.clone();
    }

    fn dispose(&mut self) {
        if let Some(id) = self.fling_listener_id.take() {
            self.fling_controller.remove_listener(id);
        }
        if let (Some(vsync), Some(registration)) =
            (self.vsync.take(), self.vsync_registration.take())
        {
            vsync.unregister(registration);
        }
        self.fling_controller.dispose();
    }
}
