//! [`BackGestureController`] and the edge-anchored swipe-back detector —
//! the iOS-style drag-to-pop substrate. Not public API: the module is
//! `pub(crate)`, nameable outside the crate only through the doc-hidden,
//! temporary `__test_access` (ADR-0083 §4).
//!
//! # Pacing
//!
//! The release animation is a fixed 350ms / `Curves::FastEaseInToSlowEaseOut`
//! settle, not a velocity-scaled lerp.
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

use flui_animation::{Animation, AnimationController, Curve, Curves};
use flui_foundation::Listenable;
use flui_interaction::recognizers::drag_variants::horizontal_drag;
use flui_interaction::{
    DragEndDetails, DragGestureRecognizer, DragStartDetails, DragUpdateDetails, GestureRecognizer,
    PointerEventExt,
};
use flui_painting::typography::TextDirection;
use flui_rendering::hit_testing::HitTestBehavior;
use flui_view::prelude::*;
use flui_view::{AnimatedView, impl_animated_view};

use super::navigator::NavigatorHandle;
use super::route::RouteId;
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
    navigator: NavigatorHandle,
    route: RouteId,
    controller: AnimationController,
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
            navigator,
            route,
            controller,
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
        let curve: Arc<dyn Curve + Send + Sync> = Arc::new(Curves::FastEaseInToSlowEaseOut); // see `PopPacing`'s doc (binding.rs) — same erased easing-curve boundary
        let is_current = self.navigator.current() == Some(self.route);
        let animate_forward = if !is_current {
            // A route already navigated away from (but perhaps still in the
            // stack) animates by whether it is still active, never by
            // velocity or drag position.
            self.navigator.route_is_active(self.route)
        } else if velocity.abs() >= MIN_FLING_VELOCITY {
            velocity <= 0.0
        } else {
            self.controller.value() > 0.5
        };

        if animate_forward {
            let _ = self.controller.animate_to_curved(
                1.0,
                Some(DROPPED_SWIPE_DURATION),
                Arc::clone(&curve),
            );
        } else {
            if is_current {
                // Reuse the navigator's pop, paced to match this gesture. The
                // pacing rides the pop command itself (`pop_paced`), reaching
                // `TransitionRoute::did_pop`'s `animate_back_curved` call
                // atomically — the controller's very first reverse run after
                // this drag uses the gesture's pacing, never a transient
                // default one (see `navigator.rs`'s `pop_paced` doc for why
                // this is not a two-step pop-then-animate-back).
                let _ = self.navigator.pop_paced(
                    self.route,
                    DROPPED_SWIPE_DURATION,
                    Arc::clone(&curve),
                );
            }
            // The pop may have finished inline if already at the target
            // destination — this covers both that case (nothing left to
            // override) and `!is_current` (no pop happened
            // above at all, but this route's own controller may still need
            // to settle toward 0).
            if self.controller.is_animating() {
                let _ =
                    self.controller
                        .animate_back_curved(0.0, Some(DROPPED_SWIPE_DURATION), curve);
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
    navigator: NavigatorHandle,
    route: RouteId,
    controller: AnimationController,
    /// Re-evaluated on **every** pointer-down, never baked at build time.
    enabled: Rc<dyn Fn() -> bool>,
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
            navigator,
            route,
            controller,
            enabled,
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

    fn on_pointer_down(
        &self,
        recognizer: &Arc<DragGestureRecognizer>,
        dispatch: flui_interaction::PointerDispatch<'_>,
    ) {
        let event = dispatch.local;
        if !(self.enabled)() {
            return;
        }
        // Multi-touch: while a drag is active, a second pointer-down in the
        // edge region must not start a second gesture — a hard guard rather
        // than a debug-only assertion.
        if self.gesture.borrow().is_some() {
            return;
        }
        // Both spaces, from the pair the Listener hands over: passing the
        // localised position twice is how a recogniser ends up reporting a
        // local position under the name `global_position` (issue #908), and
        // the transform between them is exactly what an edge-anchored back
        // gesture sits behind.
        recognizer.add_pointer(
            event.pointer_id(),
            event.position(),
            dispatch.global.position(),
        );
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
        let velocity = convert_to_logical(
            details.primary_velocity / self.normalized_width(),
            self.direction.get(),
        );
        self.finish_drag(velocity);
    }

    fn on_drag_cancel(&self) {
        // A cancel can arrive even if the drag never started, so
        // `finish_drag` is a no-op if no gesture is in flight.
        self.finish_drag(0.0);
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
#[derive(Clone)]
pub(crate) struct BackGestureDetector {
    navigator: NavigatorHandle,
    route: RouteId,
    controller: AnimationController,
    enabled: Rc<dyn Fn() -> bool>,
    child: Child,
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
            navigator,
            route,
            controller,
            enabled,
            child: Child::some(child.into_view()),
        }
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
            runtime: Rc::new(BackGestureRuntime::new(
                self.navigator.clone(),
                self.route,
                self.controller.clone(),
                Rc::clone(&self.enabled),
            )),
            recognizer: None,
        }
    }
}

pub(crate) struct BackGestureDetectorState {
    runtime: Rc<BackGestureRuntime>,
    /// Built exactly once in `init_state` against the presentation arena.
    recognizer: Option<Recognizer>,
}

struct Recognizer {
    drag: Arc<DragGestureRecognizer>,
}

impl std::fmt::Debug for BackGestureDetectorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackGestureDetectorState")
            .finish_non_exhaustive()
    }
}

impl ViewState<BackGestureDetector> for BackGestureDetectorState {
    fn init_state(&mut self, ctx: &dyn LifecycleContext) {
        self.recognizer = Some(self.build_recognizer(ctx));
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

        let recognizer = self
            .recognizer
            .as_ref()
            .expect("BUG: init_state must build the recognizer before the first build");

        let down_runtime = Rc::clone(&self.runtime);
        let down_drag = Arc::clone(&recognizer.drag);
        let move_drag = Arc::clone(&recognizer.drag);
        let up_drag = Arc::clone(&recognizer.drag);
        let cancel_drag = Arc::clone(&recognizer.drag);

        let listener = Listener::new()
            .behavior(HitTestBehavior::Translucent)
            // The drag recognizer tracks one space; hand it the local one,
            // which is what it has always received.
            .on_pointer_down(move |_cx, dispatch| {
                down_runtime.on_pointer_down(&down_drag, dispatch);
            })
            .on_pointer_move(move |_cx, dispatch| move_drag.handle_event(dispatch))
            .on_pointer_up(move |_cx, dispatch| up_drag.handle_event(dispatch))
            .on_pointer_cancel(move |_cx, dispatch| cancel_drag.handle_event(dispatch));

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
        self.runtime.dispose_safety_net();
        if let Some(recognizer) = self.recognizer.as_ref() {
            recognizer.drag.dispose();
        }
    }
}

impl BackGestureDetectorState {
    fn build_recognizer(&self, ctx: &dyn BuildContext) -> Recognizer {
        let arena = GestureArenaScope::of(ctx);

        let start_runtime = Rc::clone(&self.runtime);
        let update_runtime = Rc::clone(&self.runtime);
        let end_runtime = Rc::clone(&self.runtime);
        let cancel_runtime = Rc::clone(&self.runtime);
        let drag = horizontal_drag(arena)
            .with_on_start(move |details| start_runtime.on_drag_start(details))
            .with_on_update(move |details| update_runtime.on_drag_update(details))
            .with_on_end(move |details| end_runtime.on_drag_end(details))
            .with_on_cancel(move || cancel_runtime.on_drag_cancel());

        Recognizer { drag }
    }
}

// The tests that need a mounted navigator live in
// `crates/flui-widgets/tests/back_gesture.rs`.
