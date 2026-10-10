//! [`HeroController`] — the measurement half of the hero machinery.
//!
//! [`HeroController`] and
//! [`FlightDirection`] are public; the manifests, measurement, and flight
//! machinery below are not public API — nameable only through the doc-hidden,
//! temporary `__test_access` (ADR-0083 §4).
//!
//! This is the observer that decides *when* a flight starts, *where* its destination
//! will be, and *which heroes* fly. It records [`HeroFlightManifest`] values for
//! diagnostics/tests and hands valid ones to the private flight manager. The flight
//! itself — the overlay entry, `RectTween`, shuttle, and driving animation — lives in
//! `hero_flight.rs`.
//!
//! The pieces it stands on already exist: the private [`Hero`] view, the per-route
//! [`HeroRegistry`] behind an ambient [`HeroScope`], and [`HeroHandle`] with its
//! `start_flight` / `end_flight` placeholder machinery (`hero.rs`). The
//! controller still does not call `start_flight` directly — the private `HeroFlight`
//! does that when launched.
//!
//! [`Hero`]: super::hero::Hero
//! [`HeroRegistry`]: super::hero::HeroRegistry
//! [`HeroScope`]: super::hero::HeroScope
//! [`HeroHandle`]: super::hero::HeroHandle
//!
//! # How it decides
//!
//! * [`HeroController::did_change_top`] is the trigger. The controller reacts **only**
//!   to a change of top route, never to push/pop callbacks, which fire for routes that
//!   never become the top one;
//! * [`HeroController::maybe_start`] classifies the transition and schedules the
//!   measurement;
//! * [`MeasurementPass`] is the prologue and matching loop.
//!
//! The whole design rests on one fact: putting a route offstage changes its animation
//! value to 1.0. Once that frame completes, the heroes in the `to` route are known to
//! be at their destination, and the `to` route can go back onstage.
//!
//! So the sequence is: flip the destination offstage → let the frame build, lay out
//! and commit → measure from a post-frame callback → put it back onstage. Every
//! piece of that is a seam this ADR built, and this controller is the first thing
//! that composes them:
//!
//! | Step | Seam |
//! |---|---|
//! | flip the destination offstage | [`ModalHandle::set_offstage`] via the navigator's modal registry |
//! | a route becomes top | `Notification::TopChanged`, delivered outside the history lock |
//! | offstage ⇒ `animation.value == 1.0` | the `ModalRoute` animation proxies |
//! | measure after the frame | [`PostFrameHandle`] |
//! | the callback runs *after* layout commits | `UpdateScheduler::drive_frame` |
//! | the destination's page subtree | [`RouteSubtree`] |
//! | the subtree's committed size | `PipelineOwner::box_size` |
//! | the subtree's transform to the render root | `PipelineOwner::transform_to` |
//! | the navigator on an observer | [`NavigatorObserver::did_attach`] |
//! | all heroes of a route | per-route `HeroRegistry`, filled by registration rather than an element walk |
//! | a hero's rect in its route's space | `HeroHandle::bounding_box_in` |
//!
//! # What is deliberately absent
//!
//! The customization hooks: `Hero::create_rect_tween`,
//! `Hero::flight_shuttle_builder` (which takes no foreign `BuildContext`),
//! the state-preserving `Hero::placeholder`, and `Hero::curve` / `Hero::reverse_curve`
//! with a `Curves::FastOutSlowIn` default. `FlightDirection` is public
//! for the shuttle builder, and `HeroMode` grounds a subtree.
//!
//! The private surface stays out of the public API: `HeroTag`, `HeroRegistry`,
//! `HeroScope`, `HeroHandle`, `HeroFlightManifest`, and the flight machinery live in
//! crate-private modules, nameable only through the doc-hidden, temporary
//! `__test_access` (ADR-0083 §4).
//!
//! Cross-navigator hero matching is live: [`MeasurementPass::collect_manifests`]
//! matches against [`ModalHandle::all_heroes`](super::modal_route::ModalHandle::all_heroes),
//! which pulls in a nested `Navigator`'s current `PageRoute` heroes recursively.
//! `HeroControllerScope::none` still stops a nested navigator from auto-attaching
//! its *own* controller (so it drives no flights of its own), but does not gate
//! this matching.
//!
//! **User-gesture hero flights are live:**
//! [`did_start_user_gesture`](HeroController::did_start_user_gesture) launches a
//! flight the instant a drag begins, with a synchronous
//! measurement fast path when the destination already has a valid laid-out size;
//! [`did_stop_user_gesture`](HeroController::did_stop_user_gesture) manually
//! dismisses a flight whose drag never moved; and
//! [`Hero::transition_on_user_gestures`](super::hero::Hero::transition_on_user_gestures)
//! gates per-hero participation — a pair flies during a gesture only when both
//! ends opt in. The terminal-status deferral this all rests on lives in
//! `hero_flight.rs`.
//!
//! [`ModalHandle::set_offstage`]: super::modal_route::ModalHandle::set_offstage
//! [`PostFrameHandle`]: flui_scheduler::PostFrameHandle
//! [`RouteSubtree`]: super::subtree::RouteSubtree

// A `Navigator` now auto-attaches a `HeroController` in production, so the
// controller and its flight path are live. What stays test-only are the `pub(crate)`
// introspection accessors (`scheduled_count`, `measurements`, `manifests`), which the
// integration tests read through `crate::__test_access::HeroControllerProbe`
// (ADR-0083 §4) to assert the measurement pass.

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_animation::{Animatable, Animation, AnimationStatus, ArcCurve, Curve, CurvedAnimation};
use flui_foundation::geometry::Size;
use flui_foundation::geometry::{Matrix4, Rect};
use parking_lot::Mutex;

use super::hero::{HeroHandle, HeroTag, RectTweenFactory};
use super::hero_flight::{FlightEpoch, FlightManager, FlightPlan};
use super::lifecycle::{RetiredMap, RetiredValues, Terminal, TerminalVec};
use super::modal_route::ModalHandle;
use super::navigator::NavigatorHandle;
use super::observer::NavigatorObserver;
use super::route::RouteId;

/// Which way a flight would run.
///
/// Derived from the two routes' animation **statuses**, not from which navigator
/// call happened — a pop and a push both arrive here as a change of top route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlightDirection {
    /// `to` is arriving on top of `from`: the destination is running forward.
    Push,
    /// `from` is leaving, revealing `to` beneath it: the source is running backward.
    Pop,
}

impl FlightDirection {
    /// Classifies the transition from the user-gesture flag and the two routes'
    /// animation statuses.
    ///
    /// A user-gesture transition is unconditionally a pop, decided before
    /// either route's own status is consulted, since the gesture (an edge
    /// swipe-back) is definitionally a pop-in-progress regardless of what the
    /// drag has done to the routes' animation values so far.
    ///
    /// `None` means "neither route is transitioning", which does **not**
    /// abort: the measurement still runs.
    fn classify(
        is_user_gesture_transition: bool,
        from_status: AnimationStatus,
        to_status: AnimationStatus,
    ) -> Option<Self> {
        if is_user_gesture_transition {
            return Some(Self::Pop);
        }
        match (from_status, to_status) {
            (AnimationStatus::Reverse, _) => Some(Self::Pop),
            (_, AnimationStatus::Forward) => Some(Self::Push),
            _ => None,
        }
    }
}

/// What one post-frame measurement resolved.
///
/// The route-level measurement that precedes manifest collection and flight launch.
/// Keeping it recorded separately proves the underlying seams still compose into a
/// destination rect before that data is consumed to match hero pairs.
///
/// `pub` only so `crate::__test_access` can re-export it (ADR-0083 §4); the
/// module is private, so nothing else names it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measurement {
    /// `None` when neither route was animating; see `FlightDirection::classify`.
    pub direction: Option<FlightDirection>,
    /// The route the transition leaves.
    pub from: RouteId,
    /// The route the transition reaches.
    pub to: RouteId,
    /// The destination page subtree's committed size. `None` when the destination
    /// has not laid out — which, after a frame, would be a bug.
    pub to_size: Option<Size>,
    /// The destination page subtree's transform, taken against the render root
    /// rather than the navigator's own render object — FLUI's `Navigator` is not
    /// a render object.
    pub to_transform: Option<Matrix4>,
    /// What the destination's primary animation read *while it was offstage*. The
    /// whole mechanism is a lie unless this is `1.0`.
    pub to_animation_while_offstage: f64,
}

/// Whether a flight's two rects are usable.
///
/// This manifest type still carries concrete route-pair geometry, so both rects must be
/// finite. A non-finite rect would make the future `RectTween` interpolate
/// `NaN`/`Infinity` and paint the shuttle nowhere.
///
/// **Defensive, and known to be so.** Every rect here is built from
/// `PipelineOwner::box_size` and `transform_to`, and no reachable FLUI configuration
/// produces a non-finite one today — an unlaid-out hero is `None`, not infinite. The
/// guard exists because a future `RenderTransform` with a degenerate matrix would
/// reach it. It is unit-tested directly rather than pretended to be exercised
/// end-to-end.
pub(crate) fn is_valid_flight(from_rect: Rect, to_rect: Rect) -> bool {
    to_rect.is_finite() && from_rect.is_finite()
}

/// Everything known about a flight that *would* start, for one tag.
///
/// Carries only what a measurement produces, not everything a flight needs: no
/// `overlay`, no `create_rect_tween`, no `shuttle_builder`, no diverted flag. Both
/// rects are in their own route's coordinate space.
///
/// `pub` only so `crate::__test_access` can re-export it (ADR-0083 §4); the
/// module is private, so nothing else names it.
#[derive(Debug, Clone, PartialEq)]
pub struct HeroFlightManifest {
    /// The tag both routes share.
    pub tag: HeroTag,
    /// `None` when neither route was animating; see `FlightDirection::classify`.
    pub direction: Option<FlightDirection>,
    /// The route the hero flies from.
    pub from_route: RouteId,
    /// The route the hero flies to.
    pub to_route: RouteId,
    /// The source hero's bounding box in `from_route`'s coordinate space.
    pub from_rect: Rect,
    /// The destination hero's bounding box in `to_route`'s coordinate space.
    pub to_rect: Rect,
    /// Started by `did_start_user_gesture`, not a programmatic push/pop.
    pub is_user_gesture_transition: bool,
}

/// Watches a navigator, measures where hero flights land, and launches private flights.
///
/// Install with [`NavigatorHandle::add_observer`]. Holds no `GlobalKey`, reads no
/// element tree, and never touches the render tree from an observer callback — the
/// measurement happens in a post-frame callback, which is the only moment a route's
/// geometry is both committed and offstage.
#[derive(Default)]
pub struct HeroController {
    /// The observed navigator. `None` before
    /// attach and after detach, which is what makes a stale controller inert.
    navigator: Terminal<Mutex<Option<NavigatorHandle>>>,
    /// How many post-frame measurements have been *scheduled*. One per eligible
    /// push/pop, never one per observer callback.
    scheduled: Terminal<Arc<AtomicUsize>>,
    /// What those callbacks resolved, in order.
    measurements: Terminal<Arc<Mutex<Vec<Measurement>>>>,
    /// One per tag that both routes share and that measured to a finite rect.
    manifests: Terminal<Arc<TerminalVec<HeroFlightManifest>>>,
    /// One flight per tag in the air.
    flights: Terminal<Rc<FlightManager>>,
    /// The fallback rect-tween factory for heroes that set none of their own.
    /// `None` = linear.
    default_rect_factory: Terminal<Option<RectTweenFactory>>,
}

impl Drop for HeroController {
    fn drop(&mut self) {
        // Withdraw the complete controller before releasing any owner. Shared
        // recordings and flights retire only at their own physical last owner.
        let navigator = self.navigator.withdraw();
        let scheduled = self.scheduled.withdraw();
        let measurements = self.measurements.withdraw();
        let manifests = self.manifests.withdraw();
        let flights = self.flights.withdraw();
        let factory = self.default_rect_factory.withdraw();
        drop((
            navigator,
            scheduled,
            measurements,
            manifests,
            flights,
            factory,
        ));
    }
}

impl std::fmt::Debug for HeroController {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HeroController")
            .field("attached", &self.navigator.lock().is_some())
            .finish_non_exhaustive()
    }
}

impl HeroController {
    /// A hero controller.
    ///
    /// Most apps do not construct one: a bare `Navigator` auto-creates a default
    /// controller, and [`HeroControllerScope`](super::hero_controller_scope::HeroControllerScope)
    /// hosts an explicit controller when needed. `NavigatorHandle::add_observer` still
    /// accepts one by hand for compatibility and replaces the auto-default if it was
    /// already installed.
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// A hero controller whose flights use `factory` for any hero that sets no
    /// `create_rect_tween` of its own — e.g. Material's arc-shaped rect tween. A
    /// per-`Hero` factory still overrides this.
    #[must_use]
    pub fn with_rect_tween<F, A>(factory: F) -> Arc<Self>
    where
        F: Fn(Rect, Rect) -> A + 'static,
        A: Animatable<Value = Rect> + 'static,
    {
        let mut controller = Self::default();
        controller.default_rect_factory = Terminal::new(Some(Rc::new(move |begin, end| {
            Box::new(factory(begin, end)) as Box<dyn Animatable<Value = Rect>>
        })));
        Arc::new(controller)
    }

    /// The navigator this controller observes, or `None` when detached.
    pub(crate) fn navigator(&self) -> Option<NavigatorHandle> {
        self.navigator.lock().clone()
    }

    /// How many post-frame measurements have been scheduled.
    pub(crate) fn scheduled_count(&self) -> usize {
        self.scheduled.load(Ordering::SeqCst)
    }

    /// Everything the post-frame callbacks resolved, in order.
    pub(crate) fn measurements(&self) -> Vec<Measurement> {
        self.measurements.lock().clone()
    }

    /// The flights that started, one per shared tag. Recorded even after they land.
    pub(crate) fn manifests(&self) -> Vec<HeroFlightManifest> {
        self.manifests.lock().clone()
    }

    /// The flights currently in the air.
    pub(crate) fn flights(&self) -> &Rc<FlightManager> {
        &self.flights
    }

    /// The eligibility test, then either the gesture-pop sync fast path or the
    /// offstage-then-schedule move.
    ///
    /// Runs **inside an observer callback** (`did_change_top` or
    /// `did_start_user_gesture`), so it does exactly three kinds of work:
    /// registry lookups behind their own mutexes, a same-frame geometry read
    /// (the sync fast path only, never `history` mutation), and scheduling.
    fn maybe_start(
        &self,
        from: Option<RouteId>,
        to: Option<RouteId>,
        is_user_gesture_transition: bool,
    ) {
        let Some(navigator) = self.navigator() else {
            return; // Detached.
        };
        let epoch = self.flights.epoch();
        // No previous top route: nothing to fly from.
        let (Some(from), Some(to)) = (from, to) else {
            return;
        };
        // Only page routes fly. `TransitionGroup::Page` already encodes "is a
        // `PageRoute`", because FLUI's routes name each other by id.
        if from == to || !navigator.is_page_route(from) || !navigator.is_page_route(to) {
            return;
        }

        // The two routes' `offstage` controls, which also carry the routes'
        // animation proxies. A route that is not a `ModalRoute` published none, and
        // a disposed one withdrew it.
        let (Some(source), Some(destination)) =
            (navigator.route_modal(from), navigator.route_modal(to))
        else {
            return;
        };

        let from_animation = source.primary_animation();
        let to_animation = destination.primary_animation();
        let direction = FlightDirection::classify(
            is_user_gesture_transition,
            from_animation.status(),
            to_animation.status(),
        );

        // A flight that has already arrived is not a flight. Note the `None` arm
        // falls through: no direction still measures.
        match direction {
            Some(FlightDirection::Pop) if from_animation.value() == 0.0 => return,
            Some(FlightDirection::Push) if to_animation.value() == 1.0 => return,
            _ => {}
        }

        // The gesture-pop sync fast path: the destination route
        // maintains state and is already laid out (its subtree render box has a
        // valid, finite size), so there is nothing to wait a frame for. No
        // offstage flip, no post-frame schedule — measure and launch right now,
        // synchronously, inside this observer callback.
        if is_user_gesture_transition
            && direction == Some(FlightDirection::Pop)
            && destination.maintain_state()
            && navigator
                .route_subtree(to)
                .zip(navigator.render_tree())
                .and_then(|(subtree, owner)| owner.with(|owner| owner.box_size(subtree.render_id)))
                .is_some_and(Size::is_finite)
        {
            let pass = MeasurementPass {
                epoch,
                navigator: &navigator,
                source: &source,
                destination: &destination,
                from,
                to,
                direction,
                is_user_gesture_transition,
                flights: &self.flights,
                default_rect_factory: &self.default_rect_factory,
            };
            let started = pass.start_transition();
            self.manifests.lock().extend(started);
            return;
        }

        // Otherwise, delay measuring until the end of the next frame, using a
        // post-frame handle the navigator captured in `init_state`, never one
        // acquired from a frame phase.
        //
        // **Before the offstage flip, not after.** The post-frame capability is an
        // `Option` — absent on an unmounted navigator, or under a binding that
        // installs no post-frame handle. Flipping first and bailing here would strand
        // the destination offstage forever: nothing else ever calls
        // `set_offstage(false)`, because the only caller is the measurement we just
        // failed to schedule. Acquire, then mutate.
        let Some(post_frame) = navigator.post_frame_handle() else {
            return;
        };

        // The queued closure is an owning framework envelope too: each
        // independent capture must retain itself after another capture fails.
        let measurements = Terminal::new(Arc::clone(&self.measurements));
        let manifests = Terminal::new(Arc::clone(&self.manifests));
        let flights = Terminal::new(Rc::clone(&self.flights));
        let default_rect_factory = Terminal::new(self.default_rect_factory.clone());
        let measured_destination = Terminal::new(destination.clone());
        let navigator = Terminal::new(navigator);
        let source = Terminal::new(source);
        let schedule_result = post_frame.schedule(move |_timing| {
            let pass = MeasurementPass {
                epoch,
                navigator: &navigator,
                source: &source,
                destination: &measured_destination,
                from,
                to,
                direction,
                is_user_gesture_transition,
                flights: &flights,
                default_rect_factory: &default_rect_factory,
            };
            pass.run(&measurements, &manifests);
        });
        if let Err(error) = schedule_result {
            tracing::warn!(
                ?error,
                "hero measurement could not enter the owner-local post-frame lane"
            );
            return;
        }

        // Only hide the destination after its restoring measurement is guaranteed
        // to be queued. A failed local-lane registration must never strand it.
        destination.set_offstage(to_animation.value() == 0.0);
        self.scheduled.fetch_add(1, Ordering::SeqCst);
    }
}

/// One scheduled measurement, with everything it captured at schedule time.
///
/// This is the flight-start prologue: put the
/// destination back onstage, read the geometry the offstage frame committed, and match
/// the two routes' heroes by tag.
///
/// It runs in the **post-frame** phase of the frame the offstage flip dirtied, so
/// `box_size` and `transform_to` answer against committed layout.
/// Reading them from `did_change_top` instead would answer `None`, or worse, answer
/// with the *previous* frame's geometry.
///
/// A struct rather than a seven-argument function: it is the closure's payload, and
/// each field is one input to the flight manifest.
struct MeasurementPass<'a> {
    epoch: FlightEpoch,
    navigator: &'a NavigatorHandle,
    source: &'a ModalHandle,
    destination: &'a ModalHandle,
    from: RouteId,
    to: RouteId,
    direction: Option<FlightDirection>,
    /// Whether this transition was started by `did_start_user_gesture` rather
    /// than a programmatic push/pop — threaded into every
    /// [`HeroFlightManifest`] and used to filter heroes that did not opt in.
    is_user_gesture_transition: bool,
    flights: &'a Rc<FlightManager>,
    /// The controller-level `create_rect_tween` fallback, used for a hero that set
    /// none of its own.
    default_rect_factory: &'a Option<RectTweenFactory>,
}

/// A matched pair owns three independent opaque leaves, not one tuple whose
/// generated destruction could make their failures compete.
struct MatchedHeroes {
    manifest: Terminal<HeroFlightManifest>,
    from: Terminal<HeroHandle>,
    to: Terminal<HeroHandle>,
}

impl MeasurementPass<'_> {
    fn run(
        &self,
        measurements: &Mutex<Vec<Measurement>>,
        manifests: &Mutex<Vec<HeroFlightManifest>>,
    ) {
        // The navigator may have left the tree while we waited.
        if !self.navigator.is_mounted() {
            return;
        }

        // Read before `start_transition` restores offstage: this is the value the
        // frame under measurement actually laid out with.
        let to_animation_while_offstage = self.destination.primary_animation().value();

        let to_subtree = self.navigator.route_subtree(self.to);
        let owner = self.navigator.render_tree();

        let (to_size, to_transform) = match (to_subtree, owner) {
            (Some(subtree), Some(owner)) => owner.with(|owner| {
                let transform = owner
                    .root_id()
                    .and_then(|root| owner.transform_to(subtree.render_id, root));
                (owner.box_size(subtree.render_id), transform)
            }),
            // An unmounted destination (`maintain_state == false` and covered) or an
            // unmounted navigator: nothing to measure, and nothing to fake.
            _ => (None, None),
        };

        measurements.lock().push(Measurement {
            direction: self.direction,
            from: self.from,
            to: self.to,
            to_size,
            to_transform,
            to_animation_while_offstage,
        });

        let started = self.start_transition();
        manifests.lock().extend(started);
    }

    /// The manifest-collection-and-launch core, from putting the destination back
    /// onstage onward — shared by [`run`](Self::run) (the offstage-then-post-frame path) and
    /// `HeroController::maybe_start`'s gesture-pop sync fast path, which calls
    /// this directly with no offstage flip: the destination route is already
    /// laid out, so there is nothing to restore.
    fn start_transition(&self) -> Vec<HeroFlightManifest> {
        // A no-op when the sync fast path never flipped it. Geometry stays committed until the next layout, so
        // measuring after this is safe either way.
        self.destination.set_offstage(false);
        if !self.flights.is_current(&self.epoch) {
            return Vec::new();
        }

        // Retired flights are dropped here, outside every animation listener — see
        // `FlightManager`'s type docs for why that matters.
        self.flights.drain_retired();

        // Hand the flight manager the capability it needs to drain finished flights at
        // end-of-frame, before any of them can finish. Same handle the pass itself was
        // scheduled through, so it targets the binding's scheduler.
        self.flights
            .set_post_frame(self.navigator.post_frame_handle());

        let mut started = RetiredValues(self.collect_manifests());
        let mut recovery = flui_foundation::panic::PanicRecovery::new();
        for matched in &started.0 {
            recovery.run_with(|recovery| {
                self.launch(&matched.manifest, &matched.from, &matched.to, recovery);
            });
        }
        let mut remaining = Terminal::new(std::mem::take(&mut started.0).into_iter());
        let mut manifests = Terminal::new(Vec::new());
        for mut matched in remaining.by_ref() {
            manifests.push(matched.manifest.take_value());
        }
        recovery.finish();
        manifests.take_value()
    }

    /// Start the flight for one manifest.
    ///
    /// A manifest with **no direction** never flies. The measurement is still
    /// recorded, because a manifest is measurement data, independent of whether a
    /// flight launches.
    fn launch(
        &self,
        manifest: &HeroFlightManifest,
        from_hero: &HeroHandle,
        to_hero: &HeroHandle,
        recovery: &mut flui_foundation::panic::RecoveryScope<'_>,
    ) {
        let Some(direction) = manifest.direction else {
            return;
        };
        let Some(to_subtree) = self.navigator.route_subtree(self.to) else {
            return;
        };

        // The destination route's primary animation drives a push, the source
        // route's drives a pop. The `ModalRoute` proxy, not the raw controller — so
        // an offstage route reads `1.0`, as it must.
        let (route_animation, curve_hero) = match direction {
            FlightDirection::Push => (self.destination.primary_animation(), to_hero),
            FlightDirection::Pop => (self.source.primary_animation(), from_hero),
        };

        // Wrapped in a `CurvedAnimation` on the driving hero's `curve` — the
        // destination's for a push, the source's for a pop. The reverse curve
        // defaults to the forward curve flipped, and a manifest that diverts an
        // airborne flight carries none.
        let curve = curve_hero.curve();
        let curved = CurvedAnimation::new(route_animation, curve.clone());
        let curved = if self.flights.is_airborne(&manifest.tag) {
            curved
        } else {
            let reverse_curve = curve_hero
                .reverse_curve()
                .unwrap_or_else(|| ArcCurve::new(curve.flipped()));
            curved.with_reverse_curve(reverse_curve)
        };
        let animation: std::rc::Rc<dyn Animation<f64>> = std::rc::Rc::new(curved);

        // The destination hero's factory wins, then the controller's default, then
        // linear.
        let rect_factory = to_hero
            .rect_factory()
            .or_else(|| self.default_rect_factory.clone());

        // The destination's shuttle builder wins, then the source's, then the
        // default shuttle.
        let shuttle_builder = to_hero
            .shuttle_builder()
            .or_else(|| from_hero.shuttle_builder());

        self.flights.start(
            manifest,
            FlightPlan {
                direction,
                from_hero: Terminal::new(from_hero.clone()),
                to_hero: Terminal::new(to_hero.clone()),
                to_route_subtree: to_subtree.render_id,
                overlay: Terminal::new(self.navigator.overlay().clone()),
                animation: Terminal::new(animation),
                rect_factory: Terminal::new(rect_factory),
                shuttle_builder: Terminal::new(shuttle_builder),
                is_user_gesture_transition: self.is_user_gesture_transition,
                gesture_signal: Terminal::new(self.navigator.user_gesture_signal()),
            },
            &self.epoch,
            recovery,
        );
    }

    /// During a gesture-driven transition, a hero that did not opt into
    /// [`transition_on_user_gestures`](super::hero::Hero::transition_on_user_gestures)
    /// is excluded from the match — but not silently. It is told to drop any
    /// placeholder a *prior* flight left it in, so a gesture transition
    /// interleaved with a programmatic one never leaves a non-opted-in hero
    /// hidden. A no-op for a programmatic transition
    /// (`is_user_gesture_transition == false`), where every hero participates
    /// unconditionally.
    fn filter_for_gesture(
        &self,
        heroes: HashMap<HeroTag, HeroHandle>,
    ) -> HashMap<HeroTag, HeroHandle> {
        if !self.is_user_gesture_transition {
            return heroes;
        }
        let mut remaining = Terminal::new(heroes.into_iter());
        let mut accepted = RetiredMap(HashMap::new());
        for (tag, hero) in &mut *remaining {
            let mut tag = Terminal::new(tag);
            let mut hero = Terminal::new(hero);
            if hero.transition_on_user_gestures() {
                accepted.0.insert(tag.take_value(), hero.take_value());
            } else {
                hero.end_flight(false);
            }
        }
        std::mem::take(&mut accepted.0)
    }

    /// The matching loop, reduced to what a measurement needs: every tag both
    /// routes carry, with both bounding boxes resolved in their own route's
    /// coordinate space.
    ///
    /// Each tag on the `from` side is looked up on the `to` side; a tag on only
    /// one side has no flight. Nothing here depends on iteration order. Both sides
    /// are gathered through [`ModalHandle::all_heroes`], so a tag registered inside
    /// a nested `Navigator`'s current `PageRoute` matches exactly as one registered
    /// directly on `from`/`to`.
    fn collect_manifests(&self) -> Vec<MatchedHeroes> {
        let Some(from_subtree) = self.navigator.route_subtree(self.from) else {
            return Vec::new();
        };
        let Some(to_subtree) = self.navigator.route_subtree(self.to) else {
            return Vec::new();
        };

        let from_heroes = RetiredMap(self.filter_for_gesture(self.source.all_heroes()));
        let to_heroes = RetiredMap(self.filter_for_gesture(self.destination.all_heroes()));

        let mut manifests = RetiredValues(Vec::new());
        for (tag, from_hero) in &from_heroes.0 {
            let Some(to_hero) = to_heroes.0.get(tag) else {
                continue; // A tag on only one route is not a flight.
            };

            // A hero under a disabled `HeroMode` does not fly, and a tag missing on
            // either side is not a flight. FLUI registers the hero and skips it
            // here.
            if !from_hero.hero_mode_enabled() || !to_hero.hero_mode_enabled() {
                continue;
            }

            let (Some(from_rect), Some(to_rect)) = (
                from_hero.bounding_box_in(from_subtree.render_id),
                to_hero.bounding_box_in(to_subtree.render_id),
            ) else {
                // Unmounted, or never laid out: a hero on a route that never built
                // simply does not fly.
                tracing::debug!(?tag, "hero is not measurable; no flight");
                continue;
            };

            if !is_valid_flight(from_rect, to_rect) {
                tracing::warn!(?tag, "hero flight rect is not finite; skipping");
                continue;
            }

            manifests.0.push(MatchedHeroes {
                manifest: Terminal::new(HeroFlightManifest {
                    tag: tag.clone(),
                    direction: self.direction,
                    from_route: self.from,
                    to_route: self.to,
                    from_rect,
                    to_rect,
                    is_user_gesture_transition: self.is_user_gesture_transition,
                }),
                from: Terminal::new(from_hero.clone()),
                to: Terminal::new(to_hero.clone()),
            });
        }
        std::mem::take(&mut manifests.0)
    }
}

impl NavigatorObserver for HeroController {
    /// This observer drives hero flights; see [`NavigatorObserver::observes_hero_flights`].
    fn observes_hero_flights(&self) -> bool {
        true
    }

    /// Attach to `navigator`.
    ///
    /// **A controller cannot be shared by two mounted navigators.** If it is already attached
    /// to a still-mounted navigator, the second attach is refused and logged: the
    /// controller stays with the first (whose heroes keep flying), and the second
    /// navigator's heroes do not fly rather than the controller silently pointing at
    /// the wrong one. `did_detach` frees it for reuse.
    fn did_attach(&self, navigator: NavigatorHandle) {
        let mut slot = self.navigator.lock();
        if let Some(existing) = slot.as_ref()
            && existing.is_mounted()
            && !existing.is_same(&navigator)
        {
            tracing::warn!(
                "a HeroController cannot be shared by two Navigators; the second attach \
                 is ignored. Give each Navigator its own HeroControllerScope."
            );
            return;
        }
        *slot = Some(navigator);
    }

    /// A controller that keeps observing a detached navigator would schedule
    /// against a dead binding.
    fn did_detach(&self) {
        // Withdraw the navigator before cancellation can reenter this observer,
        // then invalidate queued measurements and retire pending/airborne flights.
        // A detached controller can no longer service a flight's end-of-flight
        // drain — the shuttle retires a flight only through a live `FlightManager`
        // — so leaving flights airborne would strand their overlay entries and
        // their shuttle's painting forever. FLUI replaces the controller through
        // `add_observer`/`remove_observer` while the navigator stays alive, so a
        // controller does not live as long as its navigator. Recorded in
        // `ARCHITECTURE.md` §18.
        let previous = Terminal::new(self.navigator.lock().take());
        self.flights.finish_all();
        drop(previous);
    }

    /// The **only** route callback this controller overrides.
    ///
    /// `did_push` / `did_pop` are the wrong hook: they fire for routes that never
    /// become the top one (a `push_and_remove_until` beneath the current top), and
    /// they do not fire when a route becomes top by having its cover popped.
    fn did_change_top(&self, top: RouteId, previous_top: Option<RouteId>) {
        // `top` is not asserted current here: notifications are delivered *outside*
        // the history lock and permit an observer to mutate the stack from a
        // callback, so a re-entrant push can leave `top` transiently not-current by
        // the time this fires. The flight path is guarded downstream anyway
        // (`route_peer`/`route_modal` return `None` for a superseded route), so a
        // stale top simply measures nothing.
        //
        // Don't trigger another flight when a pop is committed as a user gesture
        // back swipe is snapped. A finger-driven pop still fires `did_change_top` (the swipe's own commit
        // pops the route), but a user gesture stays "in progress" until the
        // settling controller reports its final status (see `back_gesture.rs`), so
        // this still observes `user_gesture_in_progress() == true` at that instant
        // and suppresses the flight — this is not a substitute for
        // `did_start_user_gesture` below (which already ran its own flight, if any,
        // the instant the drag began); it is a belt-and-braces guard
        // against a *second*, redundant flight from the programmatic-looking route
        // change the gesture's commit produces.
        if self
            .navigator()
            .is_some_and(|navigator| navigator.user_gesture_in_progress())
        {
            return;
        }
        self.maybe_start(previous_top, Some(top), false);
    }

    /// The instant a gesture (e.g. an edge swipe-back) begins, run the same
    /// eligibility-and-launch path `did_change_top` would, but pre-emptively
    /// and always classified as a pop (`FlightDirection::classify`'s
    /// `is_user_gesture_transition` arm) — this is what gives the very first
    /// drag frame a flight already in progress instead of waiting for a route
    /// change to commit.
    ///
    /// Note the argument order: the route being dragged away from is `from`, the
    /// one it would reveal is `to`, exactly `maybe_start`'s own `(from, to)` shape.
    fn did_start_user_gesture(&self, route: RouteId, previous: Option<RouteId>) {
        self.maybe_start(Some(route), previous, true);
    }

    /// Early-returns while the navigator still reports a gesture in progress
    /// — nested/overlapping gestures on the same navigator collapse to one
    /// stop, matching [`NavigatorHandle::did_stop_user_gesture`]'s own
    /// 1→0-only observer notification. Once genuinely stopped, sweeps every
    /// still-airborne, gesture-driven pop flight whose proxy never left
    /// `Dismissed` — the drag never moved, so no status transition ever fired
    /// to end it on its own — and manually dismisses each one.
    fn did_stop_user_gesture(&self) {
        let Some(navigator) = self.navigator() else {
            return;
        };
        if navigator.user_gesture_in_progress() {
            return;
        }
        self.flights.finish_stalled_gesture_pops();
    }
}

#[cfg(test)]
pub(crate) mod terminal_tests {
    use super::*;
    use std::hash::{Hash, Hasher};
    use std::io::Read;
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[derive(Clone, Debug)]
    pub(crate) struct Bomb {
        pub(crate) drops: Arc<AtomicUsize>,
        pub(crate) label: &'static str,
        pub(crate) panics: bool,
    }
    impl PartialEq for Bomb {
        fn eq(&self, other: &Self) -> bool {
            self.label == other.label
        }
    }
    impl Eq for Bomb {}
    impl Hash for Bomb {
        fn hash<H: Hasher>(&self, state: &mut H) {
            self.label.hash(state);
        }
    }
    impl Drop for Bomb {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
            assert!(!self.panics, "{}", self.label);
        }
    }
    pub(crate) fn bomb(label: &'static str, panics: bool) -> (Bomb, Arc<AtomicUsize>) {
        let drops = Arc::new(AtomicUsize::new(0));
        (
            Bomb {
                drops: drops.clone(),
                label,
                panics,
            },
            drops,
        )
    }
    pub(crate) fn factory(bomb: Bomb) -> RectTweenFactory {
        Rc::new(move |begin, end| {
            let _keep_capture = &bomb;
            Box::new(flui_animation::RectTween { begin, end })
        })
    }
    pub(crate) fn assert_failure(payload: Box<dyn std::any::Any + Send>, label: &str) {
        let actual = payload
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| payload.downcast_ref::<&str>().copied());
        assert_eq!(actual, Some(label));
    }
    // Each child contains the real final-owner path. A restored double panic or
    // lock-held destructor is reported without aborting or hanging the suite.
    pub(crate) fn children(test: &str, selected: &str, cases: &[&str]) -> Option<String> {
        if let Ok(case) = std::env::var(selected) {
            return Some(case);
        }
        let mut failures = Vec::new();
        for case in cases {
            let mut child = Command::new(std::env::current_exe().expect("test executable"))
                .args(["--exact", test, "--nocapture"])
                .env(selected, case)
                .env("RUST_BACKTRACE", "0")
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("terminal ownership child");
            let mut stdout = child.stdout.take().expect("child stdout");
            let mut stderr = child.stderr.take().expect("child stderr");
            let output = std::thread::spawn(move || {
                let mut text = String::new();
                stdout.read_to_string(&mut text).expect("child output");
                text
            });
            let errors = std::thread::spawn(move || {
                let mut text = String::new();
                stderr.read_to_string(&mut text).expect("child errors");
                text
            });
            let started = Instant::now();
            while child.try_wait().expect("child status").is_none() {
                if started.elapsed() > Duration::from_secs(10) {
                    child.kill().expect("kill terminal ownership child");
                    break;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let status = child.wait().expect("child exit");
            let stdout = output.join().expect("output reader");
            let stderr = errors.join().expect("error reader");
            if !status.success() || !stdout.contains("1 passed; 0 failed") {
                failures.push(format!("{case}: {status}\n{stdout}\n{stderr}"));
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        None
    }

    #[test]
    fn hero_controller_terminal_retirement() {
        let Some(case) = children(
            "navigator::hero_controller::terminal_tests::hero_controller_terminal_retirement",
            "FLUI_HERO_CONTROLLER_TERMINAL_CASE",
            &[
                "healthy",
                "manifest_failure",
                "competing",
                "incoming_unwind",
                "shared_manifest",
            ],
        ) else {
            return;
        };
        let incoming = case == "incoming_unwind";
        let first_failure = matches!(case.as_str(), "manifest_failure" | "competing");
        let (key, key_drops) = bomb("manifest retirement", first_failure || incoming);
        let (capture, capture_drops) = bomb("factory retirement", case == "competing" || incoming);
        let mut controller = HeroController::default();
        controller.default_rect_factory = Terminal::new(Some(factory(capture)));
        controller.manifests.lock().push(HeroFlightManifest {
            tag: HeroTag::new(flui_foundation::ValueKey::new(key)),
            direction: None,
            from_route: RouteId::next(),
            to_route: RouteId::next(),
            from_rect: Rect::ZERO,
            to_rect: Rect::ZERO,
            is_user_gesture_transition: false,
        });
        if case == "shared_manifest" {
            let alias = controller.manifests.clone();
            drop(controller);
            assert_eq!(key_drops.load(Ordering::SeqCst), 0);
            assert_eq!(capture_drops.load(Ordering::SeqCst), 1);
            assert_eq!(alias.lock().len(), 1);
            drop(alias);
            assert_eq!(key_drops.load(Ordering::SeqCst), 1);
            return;
        }
        let result = catch_unwind(AssertUnwindSafe(move || {
            let _controller = controller;
            assert!(!incoming, "incoming failure");
        }));
        if incoming {
            assert_failure(result.expect_err("incoming panic"), "incoming failure");
            assert_eq!(key_drops.load(Ordering::SeqCst), 0);
            assert_eq!(capture_drops.load(Ordering::SeqCst), 0);
        } else if first_failure {
            assert_failure(result.expect_err("manifest panic"), "manifest retirement");
            assert_eq!(key_drops.load(Ordering::SeqCst), 1);
            assert_eq!(capture_drops.load(Ordering::SeqCst), 0);
        } else {
            assert!(result.is_ok());
            assert_eq!(key_drops.load(Ordering::SeqCst), 1);
            assert_eq!(capture_drops.load(Ordering::SeqCst), 1);
        }
        // Independent healthy work still retires normally after containment.
        let (capture, drops) = bomb("recovery", false);
        let mut next = HeroController::default();
        next.default_rect_factory = Terminal::new(Some(factory(capture)));
        drop(next);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }
}
