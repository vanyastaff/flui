//! [`BackGestureController`] and the edge-anchored swipe-back detector —
//! the iOS-style drag-to-pop substrate. Not public API: the module is
//! `pub(crate)`, nameable outside the crate only through the doc-hidden,
//! temporary `__test_access` (ADR-0083 §4).
//!
//! # Pacing
//!
//! A release at least as fast as `MIN_FLING_VELOCITY` settles with a fling
//! that starts at the finger's speed, so the page keeps moving as fast as it
//! was dragged. A slower release, or a cancel, takes a fixed 350ms /
//! `Curves::FastEaseInToSlowEaseOut` settle.
//!
//! # Deferred by design
//!
//! No public detector API (this whole module is `pub(crate)`), no Cupertino
//! edge-shadow/visuals, no per-hero `Hero.transitionOnUserGestures` opt-in
//! (see `hero_controller.rs`'s doc block — every hero currently behaves as
//! `transitionOnUserGestures = false`), and no `fullscreenDialog` (FLUI has
//! no such route flag yet; opting a route into `back_gesture` is the gate
//! instead of a `!fullscreenDialog` check).
//!
//! # Ambient `Directionality`
//!
//! [`BackGestureDetectorState::build`] reads the ambient
//! [`Directionality::maybe_of`], falling back to [`TextDirection::Ltr`] when
//! there is no `Directionality` ancestor (matching every other FLUI widget
//! that reads it). The sign-normalizing conversion
//! ([`convert_to_logical`]) stays a single, independently testable function
//! rather than being inlined at each call site.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use flui_animation::{Animation, AnimationController, Curves};
use flui_foundation::Listenable;
use flui_interaction::recognizers::drag_variants::horizontal_drag;
use flui_interaction::{
    DragEndDetails, DragGestureRecognizer, DragStartDetails, DragUpdateDetails, GestureRecognizer,
};
use flui_painting::typography::TextDirection;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_view::prelude::*;
use flui_view::{AnimatedView, impl_animated_view};

use super::binding::PopPacing;
use super::navigator::NavigatorHandle;
use super::route::RouteId;
use crate::interaction::recognizer_attachment::RecognizerAttachment;
use crate::{Directionality, GestureArenaScope, Listener, Positioned, SizedBox, Stack, StackFit};

/// The width of the edge-anchored hit region that can start a drag.
pub(crate) const BACK_GESTURE_WIDTH: f64 = 20.0;

/// The minimum fling velocity that decides a release, in screen-widths per
/// second.
const MIN_FLING_VELOCITY: f64 = 1.0;

/// How long the page takes to settle after the finger is released.
const DROPPED_SWIPE_DURATION: Duration = Duration::from_millis(350);

/// Normalizes a horizontal delta/velocity fraction into pop-direction
/// coordinates (positive = toward revealing the previous route), in exactly
/// one place — see the module docs on the ambient `Directionality` read.
pub(crate) fn convert_to_logical(value: f64, direction: TextDirection) -> f64 {
    match direction {
        TextDirection::Rtl => -value,
        TextDirection::Ltr => value,
    }
}

/// A controller for an iOS-style back gesture.
///
/// Works entirely in logical fractions of the controller's own `0.0..1.0`
/// range (`0.0` = new page dismissed, `1.0` = new page fully on top).
///
/// `pub` only so `crate::__test_access` can re-export it (ADR-0083 §4); the
/// module is private, so nothing else names it.
pub struct BackGestureController {
    navigator: super::lifecycle::Terminal<NavigatorHandle>,
    route: RouteId,
    controller: super::lifecycle::Terminal<AnimationController>,
}

impl Drop for BackGestureController {
    fn drop(&mut self) {
        let navigator = self.navigator.withdraw();
        let controller = self.controller.withdraw();
        drop((navigator, controller));
    }
}

impl std::fmt::Debug for BackGestureController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackGestureController")
            .field("route", &self.route)
            .finish_non_exhaustive()
    }
}

impl BackGestureController {
    /// `navigator.did_start_user_gesture()` fires immediately, before the
    /// first `drag_update`.
    pub fn new(
        navigator: NavigatorHandle,
        route: RouteId,
        controller: AnimationController,
    ) -> Self {
        navigator.did_start_user_gesture();
        Self {
            navigator: super::lifecycle::Terminal::new(navigator),
            route,
            controller: super::lifecycle::Terminal::new(controller),
        }
    }

    /// `dragUpdate(delta)`: `controller.value -= delta`.
    ///
    /// `AnimationController::set_value` stops any active run first, so no
    /// separate `stop()` call is needed here.
    pub fn drag_update(&self, delta: f64) {
        self.controller.set_value(self.controller.value() - delta);
    }

    /// `dragEnd(velocity)`.
    ///
    /// Returns `true` if the release animation is still running and the
    /// caller must keep watching for it to settle (see
    /// `BackGestureDetectorState`'s per-rebuild poll) — `false` if it settled
    /// inline, in which case the gesture is already fully closed out
    /// (`did_stop_user_gesture` already called).
    pub fn drag_end(&self, velocity: f64) -> bool {
        self.finish(velocity, false)
    }

    fn cancel(&self) -> bool {
        self.finish(0.0, true)
    }

    fn finish(&self, velocity: f64, cancelled: bool) -> bool {
        let is_current = self.navigator.current() == Some(self.route);
        let flung = is_current && !cancelled && velocity.abs() >= MIN_FLING_VELOCITY;
        let animate_forward = if !is_current {
            // A route already navigated away from (but perhaps still in the
            // stack) animates by whether it is still active, never by
            // velocity or drag position.
            self.navigator.route_is_active(self.route)
        } else if cancelled {
            true
        } else if flung {
            velocity <= 0.0
        } else {
            self.controller.value() > 0.5
        };
        // A fling settles from the finger's speed: the controller spans one
        // screen width, so widths/s are its units/s, and a positive (pop)
        // velocity lowers its value. Anything else takes the flat pacing.
        let reached_destination =
            self.controller.value() == if animate_forward { 1.0 } else { 0.0 };
        let pacing = if flung && !reached_destination {
            PopPacing::Fling {
                velocity: -velocity,
            }
        } else {
            PopPacing::Curved {
                duration: DROPPED_SWIPE_DURATION,
                curve: Arc::new(Curves::FastEaseInToSlowEaseOut), // see `PopPacing`'s doc (binding.rs) — same erased easing-curve boundary
            }
        };

        if animate_forward {
            let _ = match pacing {
                PopPacing::Fling { velocity } => self.controller.fling(velocity),
                PopPacing::Curved { duration, curve } => {
                    self.controller
                        .animate_to_curved(1.0, Some(duration), curve)
                }
            };
        } else {
            if is_current {
                // Reuse the navigator's pop, paced to match this gesture. The
                // pacing rides the pop command itself (`pop_paced`), reaching
                // `TransitionRoute::did_pop` atomically — the controller's
                // very first reverse run after this drag uses the gesture's
                // pacing, never a transient default one (see `navigator.rs`'s
                // `pop_paced` doc for why this is not a two-step
                // pop-then-animate-back).
                let _ = self.navigator.pop_paced(self.route, pacing.clone());
            }
            // The pop may have finished inline if already at the target
            // destination — this covers both that case (nothing left to
            // override) and `!is_current` (no pop happened
            // above at all, but this route's own controller may still need
            // to settle toward 0). After a fling pop the route's own run is
            // already that fling, so it is left alone.
            if self.controller.is_animating() && !flung {
                let _ = pacing.animate_back(&self.controller);
            }
        }

        if self.controller.is_animating() {
            true
        } else {
            self.navigator.did_stop_user_gesture();
            false
        }
    }
}

/// Shared, owner-thread state a `BackGestureDetector` drives from its
/// recognizer callbacks and polls from `build`.
///
/// A plain struct behind `Rc`, not `Arc`: every field here is owner-affine
/// (`NavigatorHandle` itself is `!Send + !Sync`), and every callback that
/// touches it runs on the owner thread — a drag recognizer's callbacks are
/// `Fn(..) + 'static`, not `Send + Sync` (unlike an `AnimationController`
/// status listener, which is why the settle wait below is a poll, not a
/// second status listener; see `poll_settle`'s doc).
///
/// `pub` only so `crate::__test_access` can re-export it (ADR-0083 §4); the
/// module is private, so nothing else names it.
pub struct BackGestureRuntime {
    navigator: super::lifecycle::Terminal<NavigatorHandle>,
    route: RouteId,
    controller: super::lifecycle::Terminal<AnimationController>,
    /// Re-evaluated on **every** pointer-down, never baked at build time.
    enabled: super::lifecycle::Terminal<Rc<dyn Fn() -> bool>>,
    /// Refreshed from the ambient `Directionality` on every `build` (see
    /// `BackGestureDetectorState::build`) — `create_state` has no
    /// `BuildContext`, so this starts at the LTR fallback and is corrected
    /// before the detector's first frame is ever interactive.
    direction: Cell<TextDirection>,
    /// The in-flight gesture, if a drag has started. `None` both before the
    /// first pointer down and after `drag_end`/`drag_cancel`/`dispose` have
    /// consumed it — the multi-touch guard (`Some` blocks a second pointer
    /// from starting a second gesture) and the "nothing to watch" case share
    /// this one slot.
    gesture: RefCell<Option<BackGestureController>>,
    /// Set when `drag_end`/`drag_cancel` left the release animation running;
    /// cleared by `poll_settle` once it reports `did_stop_user_gesture`.
    awaiting_settle: Cell<bool>,
}

impl Drop for BackGestureRuntime {
    fn drop(&mut self) {
        let navigator = self.navigator.withdraw();
        let controller = self.controller.withdraw();
        let enabled = self.enabled.withdraw();
        self.awaiting_settle.set(false);
        let gesture = super::lifecycle::Terminal::new(self.gesture.get_mut().take());
        drop((navigator, controller, enabled, gesture));
    }
}

impl std::fmt::Debug for BackGestureRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackGestureRuntime")
            .field("route", &self.route)
            .field("awaiting_settle", &self.awaiting_settle.get())
            .finish_non_exhaustive()
    }
}

impl BackGestureRuntime {
    /// A runtime for `route`, driving `controller`, with no gesture in flight.
    /// `enabled` is re-evaluated on every pointer-down.
    pub fn new(
        navigator: NavigatorHandle,
        route: RouteId,
        controller: AnimationController,
        enabled: Rc<dyn Fn() -> bool>,
    ) -> Self {
        Self {
            navigator: super::lifecycle::Terminal::new(navigator),
            route,
            controller: super::lifecycle::Terminal::new(controller),
            enabled: super::lifecycle::Terminal::new(enabled),
            // No `BuildContext` here — refreshed from the ambient
            // `Directionality` on every `build` instead (see the module
            // docs and `BackGestureDetectorState::build`).
            direction: Cell::new(TextDirection::Ltr),
            gesture: RefCell::new(None),
            awaiting_settle: Cell::new(false),
        }
    }

    /// Whether a drag has started and not yet been released or disposed.
    #[must_use]
    pub fn has_gesture(&self) -> bool {
        self.gesture.borrow().is_some()
    }

    /// Whether a released drag's settle animation is still owed a
    /// `did_stop_user_gesture` report.
    #[must_use]
    pub fn awaiting_settle(&self) -> bool {
        self.awaiting_settle.get()
    }

    fn admits_new_gesture(&self) -> bool {
        if !(self.enabled)() {
            return false;
        }
        // Multi-touch: while a drag is active, a second pointer-down in the
        // edge region must not start a second gesture — a hard guard rather
        // than a debug-only assertion.
        self.gesture.borrow().is_none()
    }

    /// The recognizer's drag start: begins a gesture unless one is in flight.
    pub fn on_drag_start(&self, _details: DragStartDetails) {
        if self.gesture.borrow().is_some() {
            return;
        }
        let gesture =
            BackGestureController::new(self.navigator.clone(), self.route, self.controller.clone());
        let _prev = self.gesture.borrow_mut().replace(gesture);
    }

    fn on_drag_update(&self, details: DragUpdateDetails) {
        let delta = convert_to_logical(
            details.primary_delta / self.normalized_width(),
            self.direction.get(),
        );
        if let Some(gesture) = self.gesture.borrow().as_ref() {
            gesture.drag_update(delta);
        }
    }

    fn on_drag_end(&self, details: DragEndDetails) {
        if details.reason == flui_interaction::GestureEndReason::Cancelled {
            self.on_drag_cancel();
            return;
        }
        let velocity = convert_to_logical(
            details.fling_velocity().pixels_per_second.dx / self.normalized_width(),
            self.direction.get(),
        );
        self.finish_drag(velocity);
    }

    fn on_drag_cancel(&self) {
        // A cancel can arrive even if the drag never started, so
        // cancellation is a no-op if no gesture is in flight.
        let Some(gesture) = self.gesture.borrow_mut().take() else {
            return;
        };
        if gesture.cancel() {
            self.awaiting_settle.set(true);
        }
    }

    /// Release the in-flight gesture at `velocity` (logical screen-widths per
    /// second); a no-op if none is in flight.
    pub fn finish_drag(&self, velocity: f64) {
        let Some(gesture) = self.gesture.borrow_mut().take() else {
            return;
        };
        if gesture.drag_end(velocity) {
            self.awaiting_settle.set(true);
        }
    }

    /// Keeps the user-gesture-in-progress state true until the release
    /// animation settles, so the page transition's curve does not change
    /// mid-flight. Expressed as a poll, called from
    /// `BackGestureDetectorState::build` on every rebuild — which happens on
    /// every tick of the release animation, because that `ViewState` is an
    /// `AnimatedView` subscribed to this same controller. A genuine second
    /// status listener would need to be `Send + Sync`
    /// (`AnimationController::add_status_listener`'s bound) and could
    /// therefore never touch this owner-affine `NavigatorHandle` directly.
    pub fn poll_settle(&self) {
        if self.awaiting_settle.get() && !self.controller.is_animating() {
            self.awaiting_settle.set(false);
            self.navigator.did_stop_user_gesture();
        }
    }

    /// A dispose-time safety net for a gesture whose finger is still down
    /// when the detector unmounts (e.g. the route was swept away by a
    /// `push_and_remove_until` mid-drag): post a deferred
    /// `did_stop_user_gesture` rather than calling it synchronously from
    /// dispose (an unmount is not a frame phase the navigator's own
    /// bookkeeping expects a gesture-stop from), and only if the navigator is
    /// still mounted.
    ///
    /// Two, not one, cases owe a deferred report here — both leave the
    /// navigator's gesture count still incremented:
    ///
    /// 1. A live drag (finger still down, `self.gesture` is `Some`) that
    ///    never reached `drag_end`/`drag_cancel` at all.
    /// 2. A *released* drag whose settle animation is still running
    ///    (`drag_end` returned `true`, `self.gesture` is already `None` —
    ///    `finish_drag` always takes it — but `awaiting_settle` is `true`):
    ///    the route was swept away (e.g. by `push_and_remove_until`) or lost
    ///    the race between the pop's own settle and this detector's final
    ///    rebuild before `poll_settle` ever got to observe
    ///    `!controller.is_animating()`. Checking only `self.gesture` misses
    ///    this case entirely and leaks the count forever.
    ///
    /// Either owes the same deferred `did_stop_user_gesture`. The two cases
    /// are kept as two flags (`gesture`/`awaiting_settle`) so
    /// `poll_settle`'s cheap common case doesn't need a live controller.
    pub fn dispose_safety_net(&self) {
        let had_live_gesture = self.gesture.borrow_mut().take().is_some();
        let was_awaiting_settle = self.awaiting_settle.replace(false);
        if !had_live_gesture && !was_awaiting_settle {
            return;
        }
        let navigator = self.navigator.clone();
        match navigator.local_post_frame_handle() {
            Some(post_frame) => {
                let deferred = navigator.clone();
                let schedule_result = post_frame.schedule_local(move |_timing| {
                    if deferred.is_mounted() {
                        deferred.did_stop_user_gesture();
                    }
                });
                if schedule_result.is_err() && navigator.is_mounted() {
                    // No owner-local post-frame lane available — report now
                    // rather than leak the gesture count forever.
                    navigator.did_stop_user_gesture();
                }
            }
            None => {
                if navigator.is_mounted() {
                    navigator.did_stop_user_gesture();
                }
            }
        }
    }

    /// The route's own laid-out width. FLUI has no "my own rendered size" query off `BuildContext`, so this
    /// reads the page's committed geometry the same way
    /// `hero_controller.rs`'s `MeasurementPass::run` does: through the
    /// route's registered subtree and the navigator's render tree, live on
    /// every call — never cached, so it cannot go stale mid-gesture (e.g.
    /// across an orientation change). Floored to 1.0 so a genuinely
    /// unmeasured width (before the very first layout — a state a drag
    /// cannot start from, since hit-testing itself requires laid-out
    /// geometry) never divides a delta into infinity or NaN.
    fn normalized_width(&self) -> f64 {
        let width = self
            .navigator
            .route_subtree(self.route)
            .zip(self.navigator.render_tree())
            .and_then(|(subtree, owner)| owner.with(|owner| owner.box_size(subtree.render_id)))
            .map_or(BACK_GESTURE_WIDTH, |size| size.width);
        width.max(1.0)
    }
}

/// An edge-anchored, arena-fed detector that turns a horizontal drag inside
/// [`BACK_GESTURE_WIDTH`] of the leading edge into a
/// [`BackGestureController`]-driven pop. `pub(crate)`: no public detector API
/// is exposed yet.
pub(crate) struct BackGestureDetector {
    navigator: super::lifecycle::Terminal<NavigatorHandle>,
    route: RouteId,
    controller: super::lifecycle::Terminal<AnimationController>,
    enabled: super::lifecycle::Terminal<Rc<dyn Fn() -> bool>>,
    child: super::lifecycle::Terminal<Child>,
}

impl Clone for BackGestureDetector {
    fn clone(&self) -> Self {
        Self {
            navigator: super::lifecycle::Terminal::new(self.navigator.clone()),
            route: self.route,
            controller: super::lifecycle::Terminal::new(self.controller.clone()),
            enabled: super::lifecycle::Terminal::new(self.enabled.clone()),
            child: super::lifecycle::Terminal::new(self.child.clone()),
        }
    }
}

impl BackGestureDetector {
    /// Wrap `child` with the edge-swipe-back detector for `route`, driving
    /// `controller` (the route's own primary `AnimationController`).
    /// `enabled` is re-evaluated on every pointer-down — pass
    /// `NavigatorHandle::pop_gesture_enabled` bound to `route`, not a
    /// snapshot taken at build time.
    pub(crate) fn new(
        navigator: NavigatorHandle,
        route: RouteId,
        controller: AnimationController,
        enabled: Rc<dyn Fn() -> bool>,
        child: impl IntoView,
    ) -> Self {
        Self {
            navigator: super::lifecycle::Terminal::new(navigator),
            route,
            controller: super::lifecycle::Terminal::new(controller),
            enabled: super::lifecycle::Terminal::new(enabled),
            child: super::lifecycle::Terminal::new(Child::some(child.into_view())),
        }
    }
}

impl Drop for BackGestureDetector {
    fn drop(&mut self) {
        let navigator = self.navigator.withdraw();
        let controller = self.controller.withdraw();
        let enabled = self.enabled.withdraw();
        let child = self.child.withdraw();
        drop((navigator, controller, enabled, child));
    }
}

impl std::fmt::Debug for BackGestureDetector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackGestureDetector")
            .field("route", &self.route)
            .finish_non_exhaustive()
    }
}

impl_animated_view!(BackGestureDetector);

impl AnimatedView for BackGestureDetector {
    /// Subscribing to the route's own primary controller is what makes
    /// `poll_settle` fire promptly: every tick of a gesture-driven release
    /// animation renotifies this same controller, which reschedules this
    /// `ViewState`'s `build`.
    fn listenable(&self) -> Arc<dyn Listenable> {
        Arc::new(self.controller.clone()) as Arc<dyn Listenable>
    }
}

impl StatefulView for BackGestureDetector {
    type State = BackGestureDetectorState;

    fn create_state(&self) -> Self::State {
        BackGestureDetectorState {
            runtime: super::lifecycle::Terminal::new(Rc::new(BackGestureRuntime::new(
                self.navigator.clone(),
                self.route,
                self.controller.clone(),
                Rc::clone(&self.enabled),
            ))),
            recognizer: None,
            settings: None,
            attachment: Rc::new(RecognizerAttachment::default()),
        }
    }
}

pub(crate) struct BackGestureDetectorState {
    runtime: super::lifecycle::Terminal<Rc<BackGestureRuntime>>,
    /// Current admission owner, replaced when an authored provider changes.
    recognizer: Option<Rc<DragGestureRecognizer>>,
    settings: Option<flui_interaction::GestureSettingsProvider>,
    attachment: Rc<RecognizerAttachment<DragGestureRecognizer>>,
}

impl Drop for BackGestureDetectorState {
    fn drop(&mut self) {
        self.attachment.clear();
        let runtime = self.runtime.withdraw();
        let recognizer = super::lifecycle::Terminal::new(self.recognizer.take());
        drop((runtime, recognizer));
    }
}

impl std::fmt::Debug for BackGestureDetectorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackGestureDetectorState")
            .finish_non_exhaustive()
    }
}

impl ViewState<BackGestureDetector> for BackGestureDetectorState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        let settings = GestureArenaScope::settings_of(ctx);
        let recognizer = self.build_recognizer(ctx, settings.clone());
        self.settings = Some(settings);
        self.attachment.attach(&recognizer);
        self.recognizer = Some(recognizer);
    }

    fn did_change_dependencies(&mut self, ctx: &dyn LifecycleContext) {
        let settings = GestureArenaScope::settings_of(ctx);
        if self.settings.as_ref() != Some(&settings) {
            let incoming = self.build_recognizer(ctx, settings.clone());
            self.settings = Some(settings);
            self.attachment.attach(&incoming);
            let outgoing = self.recognizer.replace(incoming);
            if let Some(outgoing) = outgoing {
                outgoing.cancel();
            }
        }
    }

    fn build(&self, view: &BackGestureDetector, ctx: &dyn BuildContext) -> impl IntoView {
        self.runtime.poll_settle();
        // Renews the `Directionality` dependency every rebuild (the same
        // contract every `InheritedView` read follows) and keeps
        // `on_drag_update`/`on_drag_end` — plain closures with no
        // `BuildContext` of their own — reading a direction that is at most
        // one frame stale.
        self.runtime
            .direction
            .set(Directionality::maybe_of(ctx).unwrap_or(TextDirection::Ltr));

        let down_runtime = super::lifecycle::Terminal::new(Rc::clone(&self.runtime));

        let listener = Listener::new()
            .behavior(HitTestBehavior::Translucent)
            .recognizer_when(&self.attachment, move |_| down_runtime.admits_new_gesture());

        let child = view
            .child
            .clone()
            .into_inner()
            .unwrap_or_else(|| SizedBox::shrink().boxed());

        Stack::new(vec![
            child,
            Positioned::new(listener)
                .left(0.0)
                .top(0.0)
                .bottom(0.0)
                .width(BACK_GESTURE_WIDTH)
                .boxed(),
        ])
        .fit(StackFit::Passthrough)
    }

    fn dispose(&mut self) {
        self.attachment.clear();
        self.runtime.dispose_safety_net();
        if let Some(recognizer) = self.recognizer.take() {
            recognizer.cancel();
        }
    }
}

impl BackGestureDetectorState {
    fn build_recognizer(
        &self,
        ctx: &dyn LifecycleContext,
        settings: flui_interaction::GestureSettingsProvider,
    ) -> Rc<DragGestureRecognizer> {
        let arena = GestureArenaScope::of(ctx);

        let start_runtime = Rc::clone(&self.runtime);
        let update_runtime = Rc::clone(&self.runtime);
        let end_runtime = Rc::clone(&self.runtime);
        let cancel_runtime = Rc::clone(&self.runtime);
        horizontal_drag(arena)
            .settings(settings)
            .on_start(move |details| start_runtime.on_drag_start(details))
            .on_update(move |details| update_runtime.on_drag_update(details))
            .on_end(move |details| end_runtime.on_drag_end(details))
            .on_cancel(move || cancel_runtime.on_drag_cancel())
            .build()
    }
}

// The tests that need a mounted navigator live in
// `crates/flui-widgets/tests/back_gesture.rs`.
