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
//! `on_refresh` is a synchronous callback receiving `EventCx`. The widget transitions to the
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
//! v1 uses a synchronous completion model rather than a future because the view
//! layer has no async executor.

use std::{
    cell::Cell,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use flui_animation::{Animation, AnimationController, AnimationStatus, DrivenController, Vsync};
use flui_foundation::{ChangeNotifier, Listenable, ListenerCallback, ListenerId};
use flui_painting::styling::Color;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_rendering::pipeline::WeakPipelineCell;
use flui_view::prelude::StatefulView;
use flui_view::{
    BuildContext, Child, EventCx, EventOutcome, IntoView, LifecycleContext, RebuildHandle,
    RebuildReason, ViewExt, ViewState,
};

use crate::animated::VsyncScope;
use crate::scroll::scrollable::presentation_device_pixel_ratio;
use crate::scroll::single_child_scroll_view::SingleChildScrollView;
use crate::scroll::{ClampingScrollPhysics, ScrollController, ScrollMetrics, SharedScrollPhysics};
use crate::{ActivityIndicator, Center, GestureDetector, Positioned, Stack};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Default pull distance (logical pixels) required to trigger a refresh.
const DEFAULT_THRESHOLD_PX: f64 = 80.0;

/// Height of the indicator overlay while refreshing (logical pixels).
/// 56 dp, a standard FAB height.
const INDICATOR_HEIGHT_PX: f64 = 56.0;

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

/// Whether a refresh is running, and how many times that has changed.
///
/// The epoch is bumped under the same lock as the flag, so a listener that
/// reads both learns the order of the mutation it observed: notifications
/// on two threads may finish in either order, but the epoch only grows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Phase {
    refreshing: bool,
    epoch: u64,
}

impl Phase {
    fn set(&mut self, refreshing: bool) {
        if self.refreshing != refreshing {
            self.refreshing = refreshing;
            self.epoch = self.epoch.saturating_add(1);
        }
    }
}

/// The last [`Phase`] a listener acted on, advanced only forward.
#[derive(Debug)]
struct PhaseTracker(Mutex<Phase>);

impl PhaseTracker {
    /// Records `observed` if it is newer than the tracked phase; returns
    /// whether that flipped the refresh flag. An older observation finishing
    /// late is ignored, so it cannot restore a superseded phase.
    fn advance(&self, observed: Phase) -> bool {
        let mut last = self
            .0
            .lock()
            .expect("BUG: PhaseTracker is never held across a panic");
        if observed.epoch <= last.epoch {
            return false;
        }
        let flipped = last.refreshing != observed.refreshing;
        *last = observed;
        flipped
    }
}

struct RefreshControllerInner {
    pull_distance_px: Mutex<f64>,
    phase: Mutex<Phase>,
    notifier: ChangeNotifier,
}

impl std::fmt::Debug for RefreshControllerInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let is_refreshing = self.phase.lock().is_ok_and(|g| g.refreshing);
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
    inner: Rc<RefreshControllerInner>,
}

impl Default for RefreshController {
    fn default() -> Self {
        Self {
            inner: Rc::new(RefreshControllerInner {
                pull_distance_px: Mutex::new(0.0),
                phase: Mutex::new(Phase::default()),
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
        self.phase().refreshing
    }

    /// The refresh phase and its mutation epoch, read together.
    fn phase(&self) -> Phase {
        *self
            .inner
            .phase
            .lock()
            .expect("BUG: RefreshController phase mutex is never held across a panic; poisoning means a bug elsewhere already corrupted controller state")
    }

    /// The current pull distance in logical pixels.
    ///
    /// Non-zero only while the user is actively overscrolling past the top.
    /// Resets to `0.0` when the pointer lifts or a refresh begins.
    #[must_use]
    pub fn pull_distance_px(&self) -> f64 {
        *self
            .inner
            .pull_distance_px
            .lock()
            .expect("BUG: RefreshController pull_distance_px mutex is never held across a panic; poisoning means a bug elsewhere already corrupted controller state")
    }

    /// Signal that the refresh operation is complete. Hides the spinner and
    /// transitions back to idle.
    ///
    /// Calling when not refreshing is a no-op (safe to call defensively).
    pub fn finish(&self) {
        let mut guard = self
            .inner
            .phase
            .lock()
            .expect("BUG: RefreshController phase mutex is never held across a panic; poisoning means a bug elsewhere already corrupted controller state");
        guard.set(false);
        drop(guard);
        self.inner.notifier.notify_listeners();
    }

    /// An `Arc<dyn Listenable>` pointing at the same inner state.
    ///
    /// Subscribe via [`AnimatedBuilder`](crate::transitions::AnimatedBuilder) to rebuild when the refresh phase or
    /// pull distance changes.
    #[must_use]
    pub fn as_listenable(&self) -> std::rc::Rc<dyn Listenable> {
        Rc::clone(&self.inner) as std::rc::Rc<dyn Listenable>
    }

    // -- crate-internal mutation called from gesture callbacks ----------------

    pub(super) fn set_pull_distance_px(&self, distance_px: f64) {
        *self
            .inner
            .pull_distance_px
            .lock()
            .expect("BUG: RefreshController pull_distance_px mutex is never held across a panic; poisoning means a bug elsewhere already corrupted controller state") =
            distance_px;
        self.inner.notifier.notify_listeners();
    }

    /// Whether `other` is a clone of this controller (the same shared state).
    fn shares_state_with(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }

    #[cfg(test)]
    fn begin_refresh(&self) {
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        recovery.run_with(|recovery| self.begin_refresh_with_recovery(recovery));
        recovery.finish();
    }

    fn begin_refresh_with_recovery(
        &self,
        recovery: &mut flui_foundation::panic::RecoveryScope<'_>,
    ) {
        {
            let mut refreshing = self
                .inner
                .phase
                .lock()
                .expect("BUG: RefreshController phase mutex is never held across a panic; poisoning means a bug elsewhere already corrupted controller state");
            let mut pull = self
                .inner
                .pull_distance_px
                .lock()
                .expect("BUG: RefreshController pull_distance_px mutex is never held across a panic; poisoning means a bug elsewhere already corrupted controller state");
            refreshing.set(true);
            *pull = 0.0;
        }
        self.inner.notifier.notify_listeners_with_recovery(recovery);
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
///     .on_refresh(|_cx| { /* start background work */ })
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
    on_refresh: Rc<dyn Fn(&mut EventCx<'_>)>,
    /// Minimum overscroll distance (logical pixels) to trigger refresh.
    threshold_px: f64,
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
            on_refresh: Rc::new(|_cx| {}),
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
    pub fn on_refresh<R: EventOutcome>(
        mut self,
        f: impl Fn(&mut EventCx<'_>) -> R + 'static,
    ) -> Self {
        self.on_refresh = Rc::new(move |cx| f(cx).report());
        self
    }

    /// Override the pull threshold in logical pixels (default: `80.0`).
    #[must_use]
    pub fn threshold_px(mut self, threshold: f64) -> Self {
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
    recognizer_owner: Rc<()>,
    fling_authority: Rc<Cell<FlingAuthority>>,
    /// The scroll controller from the current view configuration.
    /// Updated in `did_update_view` when the caller swaps controllers.
    scroll_controller: ScrollController,
    /// Ballistic simulation driver (wide-open bounds so pixel values are never
    /// clamped by the controller itself). A value listener pushes current pixel
    /// values into `scroll_controller` each vsync tick.
    fling_controller: DrivenController,
    /// Listener ID on `fling_controller`; removed in `dispose`.
    fling_listener_id: Option<ListenerId>,
    fling_status_listener_id: Option<ListenerId>,
    /// Registry identity for gesture cancellation policy; the driver owns its seat.
    vsync: Option<Vsync>,
    /// Presentation metrics acquired before event callbacks are installed.
    pipeline: Option<WeakPipelineCell>,
    /// Schedules this element's rebuild; acquired in `init_state`.
    rebuild: Option<RebuildHandle>,
    /// The refresh controller this state listens to for phase changes, and
    /// the subscription; replaced when the view hands over another one.
    phase_subscription: Option<(RefreshController, ListenerId)>,
    /// The controller from the first configuration, subscribed in
    /// `init_state` once a rebuild handle exists.
    initial_controller: Option<RefreshController>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FlingAuthority {
    Detached,
    Attached,
    Retired,
}

impl std::fmt::Debug for RefreshIndicatorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshIndicatorState")
            .field("scroll_controller", &self.scroll_controller)
            .field("fling_registered", &self.fling_controller.is_bound())
            .finish_non_exhaustive()
    }
}

impl StatefulView for RefreshIndicator {
    type State = RefreshIndicatorState;

    fn create_state(&self) -> Self::State {
        // Unbounded: pixel values from the ballistic simulation are never
        // clamped by the controller — the simulation's own `is_done`
        // terminates the run. Unboundedness is a constructor fact (#1183),
        // not a bound value. No ticker: `Vsync` drives this controller once
        // registered.
        let fling_controller = AnimationController::builder(Duration::from_millis(1))
            .unbounded()
            .build_on(None);

        RefreshIndicatorState {
            recognizer_owner: Rc::new(()),
            fling_authority: Rc::new(Cell::new(FlingAuthority::Detached)),
            scroll_controller: self.scroll_controller.clone(),
            fling_controller,
            fling_listener_id: None,
            fling_status_listener_id: None,
            vsync: None,
            pipeline: None,
            rebuild: None,
            phase_subscription: None,
            initial_controller: Some(self.controller.clone()),
        }
    }
}

/// Calls `on_flip` whenever `controller`'s refresh flag flips relative to
/// `snapshot`, the phase the caller last built against. After registering it
/// catches up with a change made between the snapshot and the registration,
/// which no notification would report.
fn listen_for_phase_flips(
    controller: &RefreshController,
    snapshot: Phase,
    on_flip: impl Fn() + Send + Sync + 'static,
) -> ListenerId {
    let tracker = Arc::new(PhaseTracker(Mutex::new(snapshot)));
    let on_flip = Arc::new(on_flip);
    let id = controller.inner.add_listener(std::rc::Rc::new({
        let watched = controller.clone();
        let tracker = Arc::clone(&tracker);
        let on_flip = Arc::clone(&on_flip);
        move || {
            // Ordered by the mutation's epoch, not by which notification
            // finishes first.
            if tracker.advance(watched.phase()) {
                on_flip();
            }
        }
    }));
    // The lifecycle build immediately following registration reads the latest
    // phase. Catch up the tracker without scheduling inside that build scope.
    tracker.advance(controller.phase());
    id
}

impl RefreshIndicatorState {
    fn bind_vsync(&mut self, ctx: &dyn LifecycleContext) {
        let incoming = VsyncScope::maybe_of(ctx);
        let unchanged = match (&self.vsync, &incoming) {
            (None, None) => true,
            (Some(current), Some(incoming)) => current.is_same(incoming),
            _ => false,
        };
        if unchanged {
            return;
        }
        self.vsync = incoming;
        self.fling_authority.set(FlingAuthority::Detached);
        // Stop at sampled pixels before detachment can settle a finite run.
        let _ = self.fling_controller.controller().stop();
        let rebound = self.fling_controller.rebind(self.vsync.as_ref());
        self.fling_authority
            .set(if self.fling_controller.is_bound() {
                FlingAuthority::Attached
            } else {
                FlingAuthority::Detached
            });
        if let Err(error) = rebound {
            tracing::error!(%error, "RefreshIndicator lost its frame registry");
        }
    }

    fn install_fling_listener(&mut self) {
        if let Some(id) = self.fling_listener_id.take() {
            self.fling_controller.controller().remove_listener(id);
        }
        let fling = self.fling_controller.controller().clone();
        let scroll = self.scroll_controller.clone();
        self.fling_listener_id = Some(self.fling_controller.controller().add_listener(
            std::rc::Rc::new(move || {
                scroll.set_pixels(fling.value());
            }),
        ));
        if let Some(id) = self.fling_status_listener_id.take() {
            self.fling_controller
                .controller()
                .remove_status_listener(id);
        }
        let position = self.scroll_controller.position();
        self.fling_status_listener_id = Some(
            self.fling_controller
                .controller()
                .add_status_listener(Rc::new(move |status| {
                    if matches!(
                        status,
                        AnimationStatus::Completed | AnimationStatus::Dismissed
                    ) {
                        position.set_is_scrolling(false);
                    }
                })),
        );
    }

    /// Listens to `controller` and rebuilds only when its refresh phase
    /// flips: a pull-distance change alone reaches external listeners of
    /// [`RefreshController::as_listenable`] but rebuilds nothing here.
    fn subscribe_phase(&mut self, controller: &RefreshController) {
        self.unsubscribe_phase();
        let Some(rebuild) = self.rebuild.clone() else {
            return;
        };
        let id = listen_for_phase_flips(controller, controller.phase(), move || {
            rebuild.schedule(RebuildReason::StateChange);
        });
        self.phase_subscription = Some((controller.clone(), id));
    }

    fn unsubscribe_phase(&mut self) {
        if let Some((controller, id)) = self.phase_subscription.take() {
            controller.inner.remove_listener(id);
        }
    }
}

impl ViewState<RefreshIndicator> for RefreshIndicatorState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.pipeline = ctx.pipeline_owner().map(|cell| cell.downgrade());
        self.install_fling_listener();
        self.rebuild = Some(ctx.rebuild_handle());
        if let Some(controller) = self.initial_controller.take() {
            self.subscribe_phase(&controller);
        }

        self.did_change_dependencies(ctx);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        self.pipeline = ctx.pipeline_owner().map(|cell| cell.downgrade());
        self.bind_vsync(ctx);
    }

    fn build(&self, view: &RefreshIndicator, _ctx: &dyn BuildContext) -> impl IntoView {
        let pipeline_end = self.pipeline.clone();
        // Built once per refresh phase, not per scroll pixel: the viewport
        // follows the shared position itself, and every gesture callback reads
        // the controllers at event time rather than capturing a snapshot.
        let scroll_view = {
            let mut scroll_view =
                SingleChildScrollView::new().position(self.scroll_controller.position());
            if let Some(content) = view.child.clone().into_inner() {
                scroll_view = scroll_view.child(content);
            }
            scroll_view
        };
        let mut stack_children: Vec<_> = vec![scroll_view.boxed()];
        if view.controller.is_refreshing() {
            // Overlay the indicator at the very top of the content area.
            let indicator = Positioned::new(
                Center::new().child(ActivityIndicator::new().color(INDICATOR_COLOR)),
            )
            .top(0.0)
            .left(0.0)
            .right(0.0)
            .height(INDICATOR_HEIGHT_PX);
            stack_children.push(indicator.boxed());
        }

        let threshold_px = view.threshold_px;
        let fling_stop = self.fling_controller.controller().clone();
        let rc_start = view.controller.clone();
        let sc_start = self.scroll_controller.clone();
        let sc_update = self.scroll_controller.clone();
        let rc_update = view.controller.clone();
        let ph_update = view.physics.clone();
        let sc_end = self.scroll_controller.clone();
        let rc_end = view.controller.clone();
        let ph_end = view.physics.clone();
        let fc_fling = self.fling_controller.controller().clone();
        let start_authority = Rc::clone(&self.fling_authority);
        let update_authority = Rc::clone(&self.fling_authority);
        let end_authority = Rc::clone(&self.fling_authority);
        let simulation_authority = Rc::clone(&self.fling_authority);
        let on_refresh_cb = view.on_refresh.clone();
        let start_fling = move |metrics: &ScrollMetrics, velocity| {
            if simulation_authority.get() != FlingAuthority::Attached {
                return false;
            }
            let Some(sim) = ph_end.create_ballistic_simulation(metrics, velocity) else {
                return false;
            };
            // Authored physics may reenter and retire this owner or its clock.
            if simulation_authority.get() != FlingAuthority::Attached {
                return false;
            }
            fc_fling.animate_with(sim).is_ok()
        };

        GestureDetector::new()
            .recognizer_owner(Rc::clone(&self.recognizer_owner))
            .behavior(HitTestBehavior::Opaque)
            .on_pan_start(move |_cx, _details| {
                if start_authority.get() == FlingAuthority::Retired {
                    return;
                }
                // Halt any in-flight fling when the user grabs the content.
                let _ = fling_stop.stop();
                if !rc_start.is_refreshing() {
                    sc_start.position().set_is_scrolling(true);
                    rc_start.set_pull_distance_px(0.0);
                }
            })
            .on_pan_update(move |_cx, details| {
                if update_authority.get() == FlingAuthority::Retired {
                    return;
                }
                // Ignore scroll/pull updates while a refresh is in progress
                // so the indicator stays stable.
                if rc_update.is_refreshing() {
                    return;
                }
                // Positive dy (finger moving DOWN) maps to a decrease in
                // scroll offset (reveals content above).
                let raw_delta_y = details.delta.dy;
                if raw_delta_y != 0.0 {
                    sc_update
                        .position()
                        .set_user_scroll_direction(if raw_delta_y > 0.0 {
                            flui_rendering::view::ScrollDirection::Forward
                        } else {
                            flui_rendering::view::ScrollDirection::Reverse
                        });
                }
                // Pull remains outside the clamped scroll position. Consume
                // it first when the finger reverses toward ordinary scrolling.
                let proposed = sc_update.pixels() - rc_update.pull_distance_px() - raw_delta_y;

                if proposed < sc_update.min_scroll_extent() {
                    // Overscroll at top: track how far past the boundary
                    // the user has pulled; freeze the scroll at min_extent.
                    let overscroll_px = sc_update.min_scroll_extent() - proposed;
                    rc_update.set_pull_distance_px(overscroll_px);
                    sc_update.set_pixels(sc_update.min_scroll_extent());
                } else {
                    rc_update.set_pull_distance_px(0.0);
                    let metrics = ScrollMetrics::from(&sc_update.position());
                    let clamped = ph_update.apply_boundary_conditions(&metrics, proposed);
                    sc_update.set_pixels(clamped);
                }
            })
            .on_pan_end(move |cx, details| {
                if end_authority.get() == FlingAuthority::Retired {
                    sc_end.position().set_is_scrolling(false);
                    rc_end.set_pull_distance_px(0.0);
                    return;
                }
                if rc_end.is_refreshing() {
                    sc_end.position().set_is_scrolling(false);
                    return;
                }
                if details.reason == flui_interaction::GestureEndReason::Cancelled {
                    rc_end.set_pull_distance_px(0.0);
                    let metrics = ScrollMetrics::from(&sc_end.position()).with_device_pixel_ratio(
                        presentation_device_pixel_ratio(pipeline_end.as_ref()),
                    );
                    if !start_fling(&metrics, 0.0) {
                        sc_end.position().set_is_scrolling(false);
                    }
                    return;
                }
                let pull = rc_end.pull_distance_px();
                if pull >= threshold_px {
                    // Sufficient overscroll: enter refreshing state and
                    // fire the caller's callback.
                    let mut recovery = flui_foundation::panic::PanicRecovery::new();
                    recovery.run_with(|recovery| rc_end.begin_refresh_with_recovery(recovery));
                    recovery.run_with(|recovery| {
                        sc_end
                            .position()
                            .set_is_scrolling_with_recovery(false, recovery);
                    });
                    recovery.run(|| on_refresh_cb(cx));
                    recovery.finish();
                } else {
                    // Under-threshold pull: reset and start a normal fling.
                    rc_end.set_pull_distance_px(0.0);

                    // Convert pointer velocity to scroll velocity (negate:
                    // finger DOWN = positive dy → offset increases with negative delta).
                    let fling_vel_px_per_sec = -details.fling_velocity().pixels_per_second.dy;
                    let metrics = ScrollMetrics::from(&sc_end.position()).with_device_pixel_ratio(
                        presentation_device_pixel_ratio(pipeline_end.as_ref()),
                    );
                    if !start_fling(&metrics, fling_vel_px_per_sec) {
                        sc_end.position().set_is_scrolling(false);
                    }
                }
            })
            .child(Stack::new(stack_children))
    }

    fn did_update_view(&mut self, _old_view: &RefreshIndicator, new_view: &RefreshIndicator) {
        if !self
            .scroll_controller
            .position()
            .ptr_eq(&new_view.scroll_controller.position())
        {
            // A run samples the old position's metrics. Do not carry that
            // simulation into a replacement position with different bounds.
            // Retire callback authority before stopping or retargeting listeners.
            self.fling_authority.set(FlingAuthority::Retired);
            self.fling_authority = Rc::new(Cell::new(if self.fling_controller.is_bound() {
                FlingAuthority::Attached
            } else {
                FlingAuthority::Detached
            }));
            let _ = self.fling_controller.controller().stop();
            self.scroll_controller.position().set_is_scrolling(false);
            self.scroll_controller = new_view.scroll_controller.clone();
            self.recognizer_owner = Rc::new(());
            self.install_fling_listener();
        }
        if !self
            .phase_subscription
            .as_ref()
            .is_some_and(|(controller, _)| controller.shares_state_with(&new_view.controller))
        {
            self.subscribe_phase(&new_view.controller);
        }
    }

    fn dispose(&mut self) {
        self.fling_authority.set(FlingAuthority::Retired);
        self.unsubscribe_phase();
        if let Some(id) = self.fling_listener_id.take() {
            self.fling_controller.controller().remove_listener(id);
        }
        if let Some(id) = self.fling_status_listener_id.take() {
            self.fling_controller
                .controller()
                .remove_status_listener(id);
        }
        self.scroll_controller.position().set_is_scrolling(false);
        self.fling_controller.dispose();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The start notification read the refreshing phase, then stalled while
    // `finish` ran and its notification completed. When the start resumes it
    // must be ignored, or the tracker records `refreshing` while the
    // controller is idle and the next start schedules no rebuild. A private
    // seam: the public surface cannot pause one notification inside another.
    #[test]
    fn phase_tracker_follows_mutation_order() {
        let idle = Phase::default();
        let mut refreshing = idle;
        refreshing.set(true);
        let mut finished = refreshing;
        finished.set(false);
        let mut restarted = finished;
        restarted.set(true);

        let tracker = PhaseTracker(Mutex::new(idle));
        assert!(
            !tracker.advance(finished),
            "idle to idle (through a refresh) changes nothing visible"
        );
        assert!(
            !tracker.advance(refreshing),
            "the older notification finishing late must be ignored"
        );
        assert!(
            tracker.advance(restarted),
            "the next refresh must still schedule a rebuild"
        );
    }

    // `finish` ran between the subscriber's phase snapshot (refreshing) and
    // its registration, so no notification reports it. The catch-up read
    // after registration must, or the next refresh flips nothing and its
    // indicator never shows. A private seam: the public surface cannot run a
    // mutation inside the subscription.
    #[test]
    fn phase_change_before_registration_is_caught_up() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let controller = RefreshController::new();
        controller.begin_refresh();
        let stale = controller.phase();
        controller.finish();
        let flips = Arc::new(AtomicUsize::new(0));
        let id = listen_for_phase_flips(&controller, stale, {
            let flips = Arc::clone(&flips);
            move || {
                flips.fetch_add(1, Ordering::SeqCst);
            }
        });
        assert_eq!(
            flips.load(Ordering::SeqCst),
            0,
            "the following build reads the missed finish without scheduling"
        );
        controller.begin_refresh();
        assert_eq!(
            flips.load(Ordering::SeqCst),
            1,
            "the next refresh must flip"
        );
        controller.inner.remove_listener(id);
    }
}
