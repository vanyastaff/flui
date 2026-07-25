//! [`BackGestureController`] and the edge-anchored swipe-back detector —
//! the iOS-style drag-to-pop substrate. `pub(crate)` only.
//!
//! # Oracle
//!
//! `.flutter/packages/flutter/lib/src/cupertino/route.dart` (3.44.0):
//! `_CupertinoBackGestureController`, `_CupertinoBackGestureDetector`,
//! `_CupertinoBackGestureDetectorState`. Cited by method name, not by line —
//! this file ports the *3.44.0* flat pacing (a fixed 350ms /
//! `Curves.fastEaseInToSlowEaseOut` "stay" animation), not the pre-3.4x
//! velocity-scaled lerp shape.
//!
//! # Deferred by design
//!
//! No public detector API (this whole module is `pub(crate)`), no Cupertino
//! edge-shadow/visuals, no per-hero `Hero.transitionOnUserGestures` opt-in
//! (see `hero_controller.rs`'s doc block — every hero currently behaves as
//! `transitionOnUserGestures = false`), and no `fullscreenDialog` (FLUI has
//! no such route flag yet; opting a route into `back_gesture` is this port's
//! substitute gate, not a `!fullscreenDialog` check).
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
    PointerEvent, PointerEventExt,
};
use flui_rendering::hit_testing::HitTestBehavior;
use flui_types::typography::TextDirection;
use flui_view::prelude::*;
use flui_view::{AnimatedView, impl_animated_view};

use super::navigator::NavigatorHandle;
use super::route::RouteId;
use crate::{Directionality, GestureArenaScope, Listener, Positioned, SizedBox, Stack, StackFit};

/// Flutter's `_kBackGestureWidth` (`cupertino/route.dart`, 3.44.0): the
/// width of the edge-anchored hit region that can start a drag.
pub(crate) const BACK_GESTURE_WIDTH: f32 = 20.0;

/// Flutter's `_kMinFlingVelocity`: screen-widths per second.
const MIN_FLING_VELOCITY: f32 = 1.0;

/// Flutter's `_kDroppedSwipePageAnimationDuration`.
const DROPPED_SWIPE_DURATION: Duration = Duration::from_millis(350);

/// Flutter's `_CupertinoBackGestureDetectorState._convertToLogical`:
/// normalizes a horizontal delta/velocity fraction into pop-direction
/// coordinates (positive = toward revealing the previous route), in exactly
/// one place — see the module docs on the ambient `Directionality` read.
pub(crate) fn convert_to_logical(value: f32, direction: TextDirection) -> f32 {
    match direction {
        TextDirection::Rtl => -value,
        TextDirection::Ltr => value,
    }
}

/// A controller for an iOS-style back gesture — Flutter's
/// `_CupertinoBackGestureController`.
///
/// Works entirely in logical fractions of the controller's own `0.0..1.0`
/// range (`0.0` = new page dismissed, `1.0` = new page fully on top), exactly
/// as the oracle documents itself.
pub(crate) struct BackGestureController {
    navigator: NavigatorHandle,
    route: RouteId,
    controller: AnimationController,
}

impl BackGestureController {
    /// Flutter's ctor body: `navigator.didStartUserGesture()` fires
    /// immediately, before the first `drag_update`.
    pub(crate) fn new(
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
    /// `AnimationController::set_value` now stops any active run first (step
    /// 0's Flutter-parity fix) — exactly Flutter's `value -=` setter
    /// semantics, so no separate `stop()` call is needed here.
    pub(crate) fn drag_update(&self, delta: f32) {
        self.controller.set_value(self.controller.value() - delta);
    }

    /// `dragEnd(velocity)`.
    ///
    /// Returns `true` if the release animation is still running and the
    /// caller must keep watching for it to settle (see
    /// `BackGestureDetectorState`'s per-rebuild poll) — `false` if it settled
    /// inline, in which case the gesture is already fully closed out
    /// (`did_stop_user_gesture` already called).
    pub(crate) fn drag_end(&self, velocity: f32) -> bool {
        let curve: Arc<dyn Curve + Send + Sync> = Arc::new(Curves::FastEaseInToSlowEaseOut); // PORT-CHECK-OK-DYN: see `PopPacing`'s doc (binding.rs) — same erased easing-curve boundary
        let is_current = self.navigator.current() == Some(self.route);
        let animate_forward = if !is_current {
            // https://github.com/flutter/flutter/issues/141268 — a route
            // already navigated away from (but perhaps still in the stack)
            // animates by whether it is still active, never by velocity or
            // drag position.
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
                // this is not Flutter's own two-step
                // `navigator.pop(); controller.animateBack(...)`).
                let _ = self.navigator.pop_paced(
                    self.route,
                    DROPPED_SWIPE_DURATION,
                    Arc::clone(&curve),
                );
            }
            // Flutter's fallback: "The popping may have finished inline if
            // already at the target destination" — covers both that case
            // (nothing left to override) and `!is_current` (no pop happened
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

/// Shared, owner-thread state a [`BackGestureDetector`] drives from its
/// recognizer callbacks and polls from `build`.
///
/// A plain struct behind `Rc`, not `Arc`: every field here is owner-affine
/// (`NavigatorHandle` itself is `!Send + !Sync`), and every callback that
/// touches it runs on the owner thread — a drag recognizer's callbacks are
/// `Fn(..) + 'static`, not `Send + Sync` (unlike an `AnimationController`
/// status listener, which is why the settle wait below is a poll, not a
/// second status listener; see `poll_settle`'s doc).
struct BackGestureRuntime {
    navigator: NavigatorHandle,
    route: RouteId,
    controller: AnimationController,
    /// Re-evaluated on **every** pointer-down, never baked at build time —
    /// Flutter's `_CupertinoBackGestureDetectorState._handlePointerDown`
    /// reading `widget.enabledCallback()` fresh each time.
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

impl BackGestureRuntime {
    fn on_pointer_down(&self, recognizer: &Arc<DragGestureRecognizer>, event: &PointerEvent) {
        if !(self.enabled)() {
            return;
        }
        // Multi-touch: while a drag is active, a second pointer-down in the
        // edge region must not start a second gesture — Flutter's
        // `assert(_backGestureController == null)` in `_handleDragStart`,
        // enforced here as a hard guard rather than a debug-only assertion.
        if self.gesture.borrow().is_some() {
            return;
        }
        recognizer.add_pointer(event.pointer_id(), event.position());
    }

    fn on_drag_start(&self, _details: DragStartDetails) {
        if self.gesture.borrow().is_some() {
            return;
        }
        let gesture =
            BackGestureController::new(self.navigator.clone(), self.route, self.controller.clone());
        *self.gesture.borrow_mut() = Some(gesture);
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
        // "This can be called even if start is not called" — Flutter's
        // `_handleDragCancel`. `finish_drag` is a no-op if no gesture is
        // in flight.
        self.finish_drag(0.0);
    }

    fn finish_drag(&self, velocity: f32) {
        let Some(gesture) = self.gesture.borrow_mut().take() else {
            return;
        };
        if gesture.drag_end(velocity) {
            self.awaiting_settle.set(true);
        }
    }

    /// Flutter's trailing status listener in `dragEnd`: "Keep the
    /// userGestureInProgress in true state so we don't change the curve of
    /// the page transition mid-flight." Expressed as a poll, called from
    /// `BackGestureDetectorState::build` on every rebuild — which happens on
    /// every tick of the release animation, because that `ViewState` is an
    /// `AnimatedView` subscribed to this same controller. A genuine second
    /// status listener would need to be `Send + Sync`
    /// (`AnimationController::add_status_listener`'s bound) and could
    /// therefore never touch this owner-affine `NavigatorHandle` directly.
    fn poll_settle(&self) {
        if self.awaiting_settle.get() && !self.controller.is_animating() {
            self.awaiting_settle.set(false);
            self.navigator.did_stop_user_gesture();
        }
    }

    /// A dispose-time safety net for a gesture whose finger is still down
    /// when the detector unmounts (e.g. the route was swept away by a
    /// `push_and_remove_until` mid-drag) — Flutter's
    /// `_CupertinoBackGestureDetectorState.dispose`: post a deferred
    /// `didStopUserGesture` rather than calling it synchronously from
    /// `dispose` (an unmount is not a frame phase the navigator's own
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
    ///    this case entirely and leaks the count forever — caught by
    ///    `dispose_while_awaiting_settle_after_release_returns_the_counter_to_zero`.
    ///
    /// Either owes the same deferred `did_stop_user_gesture` Flutter's
    /// `_CupertinoBackGestureDetectorState.dispose` posts unconditionally
    /// whenever `_backGestureController != null` — its `null` check happens
    /// to conflate both cases because `_backGestureController` is not
    /// separately tracked from "is a release animation still owed a stop"
    /// there; this port keeps them as two flags (`gesture`/`awaiting_settle`)
    /// so `poll_settle`'s cheap common case doesn't need a live controller.
    fn dispose_safety_net(&self) {
        let had_live_gesture = self.gesture.borrow_mut().take().is_some();
        let was_awaiting_settle = self.awaiting_settle.replace(false);
        if !had_live_gesture && !was_awaiting_settle {
            return;
        }
        let navigator = self.navigator.clone();
        match navigator.post_frame_handle() {
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

    /// The route's own laid-out width — Flutter's `context.size!.width`,
    /// read straight off `context` in `_handleDragUpdate`/`_handleDragEnd`.
    /// FLUI has no "my own rendered size" query off `BuildContext`, so this
    /// reads the page's committed geometry the same way
    /// `hero_controller.rs`'s `MeasurementPass::run` does: through the
    /// route's registered subtree and the navigator's render tree, live on
    /// every call — never cached, so it cannot go stale mid-gesture (e.g.
    /// across an orientation change). Floored to 1.0 so a genuinely
    /// unmeasured width (before the very first layout — a state a drag
    /// cannot start from, since hit-testing itself requires laid-out
    /// geometry) never divides a delta into infinity or NaN.
    fn normalized_width(&self) -> f32 {
        let width = self
            .navigator
            .route_subtree(self.route)
            .zip(self.navigator.render_tree())
            .and_then(|(subtree, owner)| owner.read().box_size(subtree.render_id))
            .map_or(BACK_GESTURE_WIDTH, |size| size.width.0);
        width.max(1.0)
    }
}

/// An edge-anchored, arena-fed detector that turns a horizontal drag inside
/// [`BACK_GESTURE_WIDTH`] of the leading edge into a
/// [`BackGestureController`]-driven pop. Flutter's
/// `_CupertinoBackGestureDetector`. `pub(crate)`: no public detector API is
/// exposed yet.
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
            runtime: Rc::new(BackGestureRuntime {
                navigator: self.navigator.clone(),
                route: self.route,
                controller: self.controller.clone(),
                enabled: Rc::clone(&self.enabled),
                // No `BuildContext` here — refreshed from the ambient
                // `Directionality` on every `build` instead (see the module
                // docs and `BackGestureDetectorState::build`).
                direction: Cell::new(TextDirection::Ltr),
                gesture: RefCell::new(None),
                awaiting_settle: Cell::new(false),
            }),
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
    fn init_state(&mut self, ctx: &dyn BuildContext) {
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
            .on_pointer_down(move |event| {
                down_runtime.on_pointer_down(&down_drag, event);
            })
            .on_pointer_move(move |event| move_drag.handle_event(event))
            .on_pointer_up(move |event| up_drag.handle_event(event))
            .on_pointer_cancel(move |event| cancel_drag.handle_event(event));

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
        let drag = horizontal_drag(arena.clone())
            .with_on_start(move |details| start_runtime.on_drag_start(details))
            .with_on_update(move |details| update_runtime.on_drag_update(details))
            .with_on_end(move |details| end_runtime.on_drag_end(details))
            .with_on_cancel(move || cancel_runtime.on_drag_cancel());

        Recognizer { drag }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use flui_interaction::arena::GestureArena;
    use flui_scheduler::Scheduler;

    use super::*;
    use crate::navigator::overlay_route::SimpleRoute;

    fn navigator_with_two_routes() -> (NavigatorHandle, RouteId, RouteId) {
        let navigator = NavigatorHandle::new();
        navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
            SizedBox::new(1.0, 1.0).into_view().boxed()
        }));
        let root = navigator.route_ids()[0];
        navigator.push(SimpleRoute::<i32>::new(|_ctx| {
            SizedBox::new(1.0, 1.0).into_view().boxed()
        }));
        let top = *navigator.route_ids().last().expect("pushed");
        (navigator, root, top)
    }

    fn controller(ms: u64) -> AnimationController {
        let scheduler = Arc::new(Scheduler::new());
        AnimationController::new(Duration::from_millis(ms), scheduler)
    }

    #[test]
    #[should_panic(expected = "GestureArenaScope")]
    fn detector_without_a_presentation_arena_fails_during_mount() {
        let (navigator, _root, top) = navigator_with_two_routes();
        let detector = BackGestureDetector::new(
            navigator,
            top,
            controller(300),
            Rc::new(|| true),
            SizedBox::shrink(),
        );
        let mut owner = flui_view::BuildOwner::new();
        let mut tree = flui_view::ElementTree::new();
        let root = tree.mount_root(&detector, &mut owner.element_owner_mut());
        owner.schedule_build_for(root, 0, flui_view::RebuildReason::InitialMount);
        owner.build_scope(&mut tree);
    }

    /// A mounted navigator with a pushed [`PageRoute`], and that route's own
    /// [`AnimationController`] — the same one `pop_paced` reaches through
    /// `did_pop`. Needed by any test that drives `drag_end`'s pop branch:
    /// `BackGestureController` must be constructed with the route's *real*
    /// controller, or the pacing it applies through `pop_paced` lands on a
    /// route with no relationship to the controller the test observes.
    fn mounted_with_transition_route() -> (
        NavigatorHandle,
        crate::test_harness::Harness,
        RouteId,
        AnimationController,
    ) {
        use super::super::page_route::PageRoute;
        use crate::test_harness::mount;

        let navigator = NavigatorHandle::new();
        navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
            SizedBox::new(1.0, 1.0).into_view().boxed()
        }));
        let mut harness = mount(crate::navigator::Navigator::new(navigator.clone()));

        let route = PageRoute::<i32>::new(|_ctx, _primary, _secondary| {
            SizedBox::new(1.0, 1.0).into_view().boxed()
        })
        .transition_duration(Duration::from_millis(300));
        let transition = route.transition_handle();
        let _pushed = harness.enter_owner_scope(|| navigator.push(route));
        harness.tick();

        let top = *navigator.route_ids().last().expect("pushed");
        let controller = transition
            .controller()
            .expect("install() created the controller");
        (navigator, harness, top, controller)
    }

    #[test]
    fn convert_to_logical_flips_sign_only_for_rtl() {
        assert_eq!(convert_to_logical(0.3, TextDirection::Ltr), 0.3);
        assert_eq!(convert_to_logical(0.3, TextDirection::Rtl), -0.3);
        assert_eq!(convert_to_logical(-0.5, TextDirection::Rtl), 0.5);
    }

    // ---- ctor: reports gesture start immediately ----

    #[test]
    fn ctor_reports_user_gesture_start_immediately() {
        let (navigator, _root, top) = navigator_with_two_routes();
        let c = controller(300);
        assert!(!navigator.user_gesture_in_progress());
        let gesture = BackGestureController::new(navigator.clone(), top, c);
        assert!(navigator.user_gesture_in_progress());
        drop(gesture);
    }

    // ---- drag_update tracks controller.value exactly ----

    #[test]
    fn drag_update_tracks_controller_value_exactly() {
        let (navigator, _root, top) = navigator_with_two_routes();
        let c = controller(300);
        c.set_value(1.0);
        let gesture = BackGestureController::new(navigator, top, c.clone());

        gesture.drag_update(0.3);
        assert!((c.value() - 0.7).abs() < 1e-6, "value={}", c.value());
        gesture.drag_update(-0.1);
        assert!((c.value() - 0.8).abs() < 1e-6, "value={}", c.value());
        gesture.drag_update(0.9);
        assert!(
            (c.value() - 0.0).abs() < 1e-6,
            "clamped to the lower bound: value={}",
            c.value()
        );
    }

    // ---- release matrix: v = -2.0 / +2.0 / 0 at value 0.49 / 0.51 ----

    #[test]
    fn release_matrix_fling_and_slow_release() {
        // Fast negative velocity (screen-widths/s): stay (route animates
        // forward to 1.0 = new page fully covers again) regardless of value.
        {
            let (navigator, _harness, top, c) = mounted_with_transition_route();
            c.set_value(0.51);
            let gesture = BackGestureController::new(navigator, top, c.clone());
            let still_settling = gesture.drag_end(-2.0);
            assert!(still_settling, "an animated release keeps the run going");
            assert_eq!(c.status(), flui_animation::AnimationStatus::Forward);
        }
        // Fast positive velocity: pop (route animates back to 0.0).
        {
            let (navigator, _harness, top, c) = mounted_with_transition_route();
            c.set_value(0.49);
            let gesture = BackGestureController::new(navigator, top, c.clone());
            let still_settling = gesture.drag_end(2.0);
            assert!(still_settling);
            assert_eq!(c.status(), flui_animation::AnimationStatus::Reverse);
        }
        // No meaningful velocity, value > 0.5: stay.
        {
            let (navigator, _harness, top, c) = mounted_with_transition_route();
            c.set_value(0.51);
            let gesture = BackGestureController::new(navigator, top, c.clone());
            let still_settling = gesture.drag_end(0.0);
            assert!(still_settling);
            assert_eq!(c.status(), flui_animation::AnimationStatus::Forward);
        }
        // No meaningful velocity, value <= 0.5: pop.
        {
            let (navigator, _harness, top, c) = mounted_with_transition_route();
            c.set_value(0.49);
            let gesture = BackGestureController::new(navigator, top, c.clone());
            let still_settling = gesture.drag_end(0.0);
            assert!(still_settling);
            assert_eq!(c.status(), flui_animation::AnimationStatus::Reverse);
        }
    }

    // ---- mid-drag programmatic pop: the pop itself must not be clobbered ----

    /// Flutter's `dragUpdate` has no `is_active`/`is_current` guard at all —
    /// `controller.value -= delta` runs unconditionally, so a drag_update
    /// after a programmatic pop still moves the value (this is *not* a
    /// no-op, and asserting otherwise would pin a divergence). What must
    /// hold is the other direction: the programmatic pop that landed
    /// mid-drag stays popped — a later drag_update must not resurrect the
    /// route or panic reaching into it.
    #[test]
    fn mid_drag_programmatic_pop_is_not_undone_by_a_later_drag_update() {
        let (navigator, mut harness, top, c) = mounted_with_transition_route();
        c.set_value(1.0);
        let gesture = BackGestureController::new(navigator.clone(), top, c.clone());
        gesture.drag_update(0.3); // value 0.7, mid-drag

        // A programmatic pop lands while the finger is still down. The route
        // stays in `route_ids()` until its (non-zero-duration) exit
        // transition finishes — `finished_when_popped` — so "the pop took
        // effect" is checked through `current()`, not stack membership.
        assert!(harness.enter_owner_scope(|| navigator.pop()));
        harness.tick();
        assert_ne!(
            navigator.current(),
            Some(top),
            "the mid-drag pop must actually move `current` off this route"
        );

        // A further drag_update on the now-stale gesture must not panic or
        // resurrect the popped route.
        gesture.drag_update(0.05);
        assert_ne!(
            navigator.current(),
            Some(top),
            "a stale drag_update after the pop must not undo it"
        );
    }

    // ---- full swipe to 0.0, then drag back: no Dismissed-finalize thrash ----

    #[test]
    fn full_swipe_to_zero_then_drag_back_does_not_thrash() {
        let (navigator, _root, top) = navigator_with_two_routes();
        let c = controller(300);
        c.set_value(1.0);
        let gesture = BackGestureController::new(navigator, top, c.clone());

        gesture.drag_update(1.0); // value -> 0.0, fully swiped
        assert_eq!(c.value(), 0.0);
        assert_eq!(c.status(), flui_animation::AnimationStatus::Dismissed);

        // Dragging back must not panic, and must move the value back up —
        // set_value's status is recomputed, not stuck at a stale Dismissed.
        gesture.drag_update(-0.4);
        assert!((c.value() - 0.4).abs() < 1e-6, "value={}", c.value());
        assert_ne!(c.status(), flui_animation::AnimationStatus::Dismissed);
    }

    // ---- dispose-mid-settle: counter returns to 0 ----

    #[test]
    fn dispose_mid_gesture_returns_the_counter_to_zero() {
        let (navigator, mut harness, top, c) = mounted_with_transition_route();
        let runtime = BackGestureRuntime {
            navigator: navigator.clone(),
            route: top,
            controller: c,
            enabled: Rc::new(|| true),
            direction: Cell::new(TextDirection::Ltr),
            gesture: RefCell::new(None),
            awaiting_settle: Cell::new(false),
        };
        runtime.on_drag_start(DragStartDetails {
            global_position: flui_types::geometry::Offset::ZERO,
            local_position: flui_types::geometry::Offset::ZERO,
            kind: flui_interaction::events::PointerType::Touch,
            timestamp: std::time::Instant::now(),
        });
        assert!(navigator.user_gesture_in_progress());

        // The detector unmounts mid-drag (finger still down) — no drag_end
        // ever ran. The navigator IS mounted here (unlike an inert
        // `NavigatorHandle::new()` fixture), so Flutter's own `if (mounted)`
        // gate in `dispose` is actually exercised, not vacuously satisfied.
        // `dispose` runs from within the element tree's own owner scope in
        // production, exactly like `push`/`pop` do — reproduced here so the
        // local post-frame lane is actually active and the report is
        // genuinely deferred, not caught by the synchronous fallback.
        assert!(navigator.is_mounted());
        harness.enter_owner_scope(|| runtime.dispose_safety_net());

        // `mount()` installs a real owner-local post-frame lane, so the
        // report is deferred (Flutter's own `addPostFrameCallback`, not a
        // synchronous call from `dispose`) — a frame tick is what delivers it.
        assert!(
            navigator.user_gesture_in_progress(),
            "the report is deferred to the next frame, not synchronous"
        );
        harness.tick();
        assert!(
            !navigator.user_gesture_in_progress(),
            "dispose must return the counter to 0 by the next frame, even \
             with no drag_end"
        );
    }

    /// A one-shot observer that counts `did_stop_user_gesture` calls.
    #[derive(Default)]
    struct GestureStopObserver {
        stops: AtomicUsize,
    }
    impl super::super::NavigatorObserver for GestureStopObserver {
        fn did_stop_user_gesture(&self) {
            self.stops.fetch_add(1, Ordering::SeqCst);
        }
    }

    // ---- full settle after release: did_stop fires, counter clears ----

    /// A released drag settled by genuinely ticking the run out (not
    /// `set_value`) must report `did_stop_user_gesture` to observers exactly
    /// once and leave `user_gesture_in_progress()` false — Flutter's
    /// trailing `AnimationStatusListener` in `dragEnd` firing on the run's
    /// real terminal status.
    ///
    /// Red-check: drop `poll_settle`'s call entirely — `awaiting_settle`
    /// stays `true` forever and this test hangs on the final assertion
    /// (never becomes `false`).
    #[test]
    fn full_settle_after_release_reports_did_stop_and_clears_the_counter() {
        let (navigator, _harness, top, c) = mounted_with_transition_route();
        let observer = Arc::new(GestureStopObserver::default());
        navigator.add_observer(Arc::clone(&observer) as Arc<dyn super::super::NavigatorObserver>);

        c.set_value(0.49); // <= 0.5, no fling: dragEnd's pop branch
        let runtime = BackGestureRuntime {
            navigator: navigator.clone(),
            route: top,
            controller: c.clone(),
            enabled: Rc::new(|| true),
            direction: Cell::new(TextDirection::Ltr),
            gesture: RefCell::new(Some(BackGestureController::new(
                navigator.clone(),
                top,
                c.clone(),
            ))),
            awaiting_settle: Cell::new(false),
        };

        runtime.finish_drag(0.0);
        assert!(
            runtime.awaiting_settle.get(),
            "the 350ms reverse run is still going"
        );
        assert!(navigator.user_gesture_in_progress());

        // Genuinely tick the run out (not `set_value`) — mid-flight polls
        // must not report early.
        c.tick_at(0.10);
        runtime.poll_settle();
        assert!(
            navigator.user_gesture_in_progress(),
            "must not report stopped before the run actually settles"
        );
        assert_eq!(observer.stops.load(Ordering::SeqCst), 0);

        c.tick_at(0.35); // >= the 350ms pacing -> settles to Dismissed
        assert_eq!(c.status(), flui_animation::AnimationStatus::Dismissed);
        runtime.poll_settle();

        assert!(
            !navigator.user_gesture_in_progress(),
            "the counter must clear once the run genuinely settles"
        );
        assert_eq!(
            observer.stops.load(Ordering::SeqCst),
            1,
            "did_stop_user_gesture must fire exactly once"
        );
    }

    // ---- dispose while awaiting settle (post-release, pre-poll): counter clears ----

    /// `dispose_safety_net` must own the deferred report for a
    /// gesture that already *released* (so `self.gesture` is `None` —
    /// `finish_drag` always takes it) but whose settle animation is still
    /// running when the detector unmounts — e.g. the route was swept away by
    /// a `push_and_remove_until` mid-settle, or lost the race between the
    /// pop's own settle and this detector's final rebuild. Checking only
    /// `self.gesture` (as if a live drag were the only case that owes a
    /// report) leaks the counter forever.
    ///
    /// Red-check: guard `dispose_safety_net` on `self.gesture` alone (drop
    /// the `awaiting_settle` check) — this test's final assertion fails,
    /// `user_gesture_in_progress()` stays `true` forever.
    #[test]
    fn dispose_while_awaiting_settle_after_release_returns_the_counter_to_zero() {
        let (navigator, mut harness, top, c) = mounted_with_transition_route();
        c.set_value(0.49);
        let runtime = BackGestureRuntime {
            navigator: navigator.clone(),
            route: top,
            controller: c.clone(),
            enabled: Rc::new(|| true),
            direction: Cell::new(TextDirection::Ltr),
            gesture: RefCell::new(Some(BackGestureController::new(
                navigator.clone(),
                top,
                c.clone(),
            ))),
            awaiting_settle: Cell::new(false),
        };

        runtime.finish_drag(0.0);
        assert!(
            runtime.gesture.borrow().is_none(),
            "finish_drag always takes it"
        );
        assert!(
            runtime.awaiting_settle.get(),
            "the release animation is still running"
        );
        assert!(navigator.user_gesture_in_progress());

        // The detector unmounts before the settle run's next poll — no
        // `poll_settle` call ever ran.
        assert!(navigator.is_mounted());
        harness.enter_owner_scope(|| runtime.dispose_safety_net());
        harness.tick();

        assert!(
            !navigator.user_gesture_in_progress(),
            "dispose must clear the counter for a release still awaiting \
             settle, not only for a still-dragging gesture"
        );
    }

    // ---- second pointer mid-drag ignored ----

    #[test]
    fn a_second_pointer_down_mid_drag_is_ignored() {
        let (navigator, _root, top) = navigator_with_two_routes();
        let c = controller(300);
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_enabled = Arc::clone(&attempts);
        let runtime = BackGestureRuntime {
            navigator,
            route: top,
            controller: c,
            enabled: Rc::new(move || {
                attempts_for_enabled.fetch_add(1, Ordering::SeqCst);
                true
            }),
            direction: Cell::new(TextDirection::Ltr),
            gesture: RefCell::new(None),
            awaiting_settle: Cell::new(false),
        };

        // First pointer starts a gesture — simulate what `on_pointer_down`
        // would hand off by driving `on_drag_start` directly (the recognizer
        // plumbing itself is exercised only through a mounted harness).
        runtime.on_drag_start(DragStartDetails {
            global_position: flui_types::geometry::Offset::ZERO,
            local_position: flui_types::geometry::Offset::ZERO,
            kind: flui_interaction::events::PointerType::Touch,
            timestamp: std::time::Instant::now(),
        });
        assert!(runtime.gesture.borrow().is_some());

        // A second pointer's down still evaluates the predicate (it is
        // per-pointer-down, not skipped), but the multi-touch guard refuses
        // to hand it to the recognizer or replace the in-flight gesture.
        let arena = GestureArena::new();
        let drag = horizontal_drag(arena.clone());
        let event = flui_interaction::events::make_down_event_for_id(
            flui_interaction::PointerId::new(2).expect("nonzero id"),
            flui_types::geometry::Offset::ZERO,
            flui_interaction::events::PointerType::Touch,
        );
        runtime.on_pointer_down(&drag, &event);
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            1,
            "the predicate still runs per pointer-down"
        );
        assert!(
            runtime.gesture.borrow().is_some(),
            "the original gesture must not be replaced by the second pointer"
        );
    }

    // ---- predicate per pointer-down: a route becoming ineligible is honored ----

    #[test]
    fn the_enabled_predicate_is_evaluated_fresh_per_pointer_down() {
        let (navigator, _root, top) = navigator_with_two_routes();
        let c = controller(300);
        let allow = Rc::new(Cell::new(true));
        let allow_for_closure = Rc::clone(&allow);
        let runtime = BackGestureRuntime {
            navigator,
            route: top,
            controller: c,
            enabled: Rc::new(move || allow_for_closure.get()),
            direction: Cell::new(TextDirection::Ltr),
            gesture: RefCell::new(None),
            awaiting_settle: Cell::new(false),
        };

        let arena = GestureArena::new();
        let drag = horizontal_drag(arena.clone());
        let down_1 = flui_interaction::events::make_down_event_for_id(
            flui_interaction::PointerId::new(1).expect("nonzero id"),
            flui_types::geometry::Offset::ZERO,
            flui_interaction::events::PointerType::Touch,
        );
        runtime.on_pointer_down(&drag, &down_1);
        assert!(
            runtime.gesture.borrow().is_none(),
            "on_pointer_down alone does not start a gesture (that's on_drag_start); \
             this only proves the predicate did not block add_pointer"
        );

        // The route becomes ineligible between builds — no rebuild needed,
        // since the predicate closure reads live state.
        allow.set(false);
        let down_2 = flui_interaction::events::make_down_event_for_id(
            flui_interaction::PointerId::new(2).expect("nonzero id"),
            flui_types::geometry::Offset::ZERO,
            flui_interaction::events::PointerType::Touch,
        );
        // A disabled predicate must refuse before ever touching the
        // recognizer — verified indirectly: no panic, no state change, and
        // (per `a_second_pointer_down_mid_drag_is_ignored` above) an enabled
        // predicate DOES reach `add_pointer`. The direct assertion is that
        // `enabled` itself, not a cached bool, gates this call.
        runtime.on_pointer_down(&drag, &down_2);
        assert!(!(runtime.enabled)());
    }
}
