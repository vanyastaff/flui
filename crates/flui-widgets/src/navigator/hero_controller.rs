//! [`HeroController`] — the measurement half of Flutter's hero machinery.
//!
//! [`HeroController`] and
//! [`FlightDirection`] are public; the manifests, measurement, and flight
//! machinery below stay `pub(crate)`.
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
//! # What this is a port of
//!
//! `.flutter/packages/flutter/lib/src/widgets/heroes.dart`, master
//! `3.33.0-0.0.pre-6280-g88e87cd963f`:
//!
//! * `HeroController.didChangeTop` (`:853-869`) → [`HeroController::did_change_top`].
//!   Note it overrides **only** `didChangeTop` — never `didPush`/`didPop`, which a
//!   port is tempted to reach for and which fire for routes that never become the
//!   top one;
//! * `_maybeStartHeroTransition` (`:911-976`) → [`HeroController::maybe_start`];
//! * `_startHeroTransition`'s prologue and matching loop (`:979-1060`) →
//!   [`MeasurementPass`].
//!
//! The whole design rests on one comment (`:964-966`):
//!
//! > *Putting a route offstage changes its animation value to 1.0. Once this frame
//! > completes, we'll know where the heroes in the `to` route are going to end up,
//! > and the `to` route will go back onstage.*
//!
//! So the sequence is: flip the destination offstage → let the frame build, lay out
//! and commit → measure from a post-frame callback → put it back onstage. Every
//! piece of that is a seam this ADR built, and this controller is the first thing
//! that composes them:
//!
//! | Step | Seam |
//! |---|---|
//! | `toRoute.offstage = …` (`:967`) | [`ModalHandle::set_offstage`] via the navigator's modal registry |
//! | `didChangeTop` (`navigator.dart:4590-4596`) | `Notification::TopChanged`, delivered outside the history lock |
//! | offstage ⇒ `animation.value == 1.0` (`routes.dart:1958`) | the `ModalRoute` animation proxies |
//! | `addPostFrameCallback` (`:968`) | [`PostFrameHandle`] |
//! | the callback runs *after* layout commits | `Scheduler::drive_frame` |
//! | `to.subtreeContext` (`:1014`) | [`RouteSubtree`] |
//! | `subtreeContext.findRenderObject()!.size` (`:952`) | `PipelineOwner::box_size` |
//! | `getTransformTo(navigatorRenderObject)` (`:1029`) | `PipelineOwner::transform_to` |
//! | `navigator` on an observer (`navigator.dart:779`) | [`NavigatorObserver::did_attach`] |
//! | `Hero._allHeroesFor(subtreeContext)` (`:1014`) | per-route `HeroRegistry`, filled by registration rather than an element walk |
//! | `_boundingBoxFor(hero, route.subtreeContext)` (`:501-509`) | `HeroHandle::bounding_box_in` |
//!
//! # What is deliberately absent
//!
//! The customization hooks: `Hero::create_rect_tween`,
//! `Hero::flight_shuttle_builder` (with the no-foreign-`BuildContext` divergence),
//! FLUI's state-preserving `Hero::placeholder` (in place of Flutter's lossy
//! `placeholderBuilder`), and `Hero::curve` / `Hero::reverse_curve` with Flutter's
//! `Curves.fastOutSlowIn` default (`heroes.dart:181`). `FlightDirection` is public
//! for the shuttle builder, and `HeroMode` grounds a subtree.
//!
//! The private surface stays private: `HeroTag`, `HeroRegistry`, `HeroScope`,
//! `HeroHandle`, `HeroFlightManifest`, and the flight machinery are `pub(crate)`, and
//! `navigator_tests::public_no_internal_route_stack_exports` fails if any is exported.
//!
//! Cross-navigator hero matching is live: [`MeasurementPass::collect_manifests`]
//! matches against [`ModalHandle::all_heroes`](super::modal_route::ModalHandle::all_heroes),
//! which pulls in a nested `Navigator`'s current `PageRoute` heroes recursively —
//! `Hero._allHeroesFor`'s nested-`Navigator` branch (`heroes.dart:317-333`).
//! `HeroControllerScope::none` still stops a nested navigator from auto-attaching
//! its *own* controller (so it drives no flights of its own), but does not gate
//! this matching — Flutter's predicate does not consult it either.
//!
//! **User-gesture hero flights are live**, closing the last Flutter parity gap:
//! [`did_start_user_gesture`](HeroController::did_start_user_gesture) launches a
//! flight the instant a drag begins (`heroes.dart:871-880`), with a synchronous
//! measurement fast path when the destination already has a valid laid-out size
//! (`_maybeStartHeroTransition`'s `hasValidSize` branch, `:948-959`);
//! [`did_stop_user_gesture`](HeroController::did_stop_user_gesture) manually
//! dismisses a flight whose drag never moved (`:882-907`); and
//! [`Hero::transition_on_user_gestures`](super::hero::Hero::transition_on_user_gestures)
//! gates per-hero participation (`heroes.dart:281-311`'s `_allHeroesFor` filter) —
//! a pair flies during a gesture only when both ends opt in. The terminal-status
//! deferral this all rests on lives in `hero_flight.rs`
//! (`_HeroFlight._handleAnimationUpdate`, `:622-650`).
//!
//! [`ModalHandle::set_offstage`]: super::modal_route::ModalHandle::set_offstage
//! [`PostFrameHandle`]: flui_scheduler::PostFrameHandle
//! [`RouteSubtree`]: super::subtree::RouteSubtree

// A `Navigator` now auto-attaches a `HeroController` in production, so the
// controller and its flight path are live. What stays test-only are the `pub(crate)`
// introspection accessors (`scheduled_count`, `measurements`, `manifests`) the tests
// read to assert the measurement pass; their `dead_code` in a non-test build cascades
// into the `ModalHandle` / `RouteBinding` / `HeroRegistry` seams. The allow keeps a
// seam from being deleted and re-derived later, out of step with the design.
#![allow(dead_code)]

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_animation::{Animatable, Animation, AnimationStatus, ArcCurve, Curve, CurvedAnimation};
use flui_geometry::{Matrix4, Rect};
use flui_types::Size;
use parking_lot::Mutex;

use super::hero::{HeroHandle, HeroTag, RectTweenFactory};
use super::hero_flight::{FlightManager, FlightPlan};
use super::modal_route::ModalHandle;
use super::navigator::NavigatorHandle;
use super::observer::NavigatorObserver;
use super::route::RouteId;

/// Which way a flight would run. Flutter's `HeroFlightDirection` (`heroes.dart:57`).
///
/// Derived from the two routes' animation **statuses**, not from which navigator
/// call happened (`heroes.dart:924-932`) — a pop and a push both arrive here as a
/// change of top route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlightDirection {
    /// `to` is arriving on top of `from`: the destination is running forward.
    Push,
    /// `from` is leaving, revealing `to` beneath it: the source is running backward.
    Pop,
}

impl FlightDirection {
    /// `switch ((isUserGestureTransition, oldRouteAnimation.status,
    /// newRouteAnimation.status))` (`heroes.dart:924-932`).
    ///
    /// A user-gesture transition is unconditionally a pop — Flutter's
    /// `case (true, _, _): flightType = HeroFlightDirection.pop` fires before
    /// either route's own status is even consulted, since the gesture (an edge
    /// swipe-back) is definitionally a pop-in-progress regardless of what the
    /// drag has done to the routes' animation values so far.
    ///
    /// `None` means "neither route is transitioning" — Flutter's `default: flightType
    /// = null`, which does **not** abort: the measurement still runs (`:934-976`).
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
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Measurement {
    /// `None` when neither route was animating; see [`FlightDirection::classify`].
    pub(crate) direction: Option<FlightDirection>,
    pub(crate) from: RouteId,
    pub(crate) to: RouteId,
    /// `to.subtreeContext.findRenderObject()!.size` (`heroes.dart:952`). `None` when
    /// the destination has not laid out — which, after a frame, would be a bug.
    pub(crate) to_size: Option<Size>,
    /// `to.subtreeContext.findRenderObject()!.getTransformTo(navigatorRenderObject)`
    /// (`heroes.dart:1029`), taken against the render root rather than the
    /// navigator's own render object — FLUI's `Navigator` is not a render object.
    pub(crate) to_transform: Option<Matrix4>,
    /// What the destination's primary animation read *while it was offstage*. The
    /// whole mechanism is a lie unless this is `1.0` (`routes.dart:1958`).
    pub(crate) to_animation_while_offstage: f32,
}

/// `_HeroFlightManifest.isValid` (`heroes.dart:530`):
///
/// ```dart
/// late final bool isValid = toHeroLocation.isFinite && (isDiverted || fromHeroLocation.isFinite);
/// ```
///
/// This manifest type still carries concrete route-pair geometry, so both rects must be
/// finite. A non-finite rect would make the future `RectTween` interpolate
/// `NaN`/`Infinity` and paint the shuttle nowhere.
///
/// **Defensive, and known to be so.** Every rect here is built from
/// `PipelineOwner::box_size` and `transform_to`, and no reachable FLUI configuration
/// produces a non-finite one today — an unlaid-out hero is `None`, not infinite. The
/// guard is ported because `isValid` exists in Flutter and because a future
/// `RenderTransform` with a degenerate matrix would reach it. It is unit-tested
/// directly rather than pretended to be exercised end-to-end.
pub(crate) fn is_valid_flight(from_rect: Rect, to_rect: Rect) -> bool {
    to_rect.is_finite() && from_rect.is_finite()
}

/// Everything known about a flight that *would* start, for one tag.
///
/// Flutter's `_HeroFlightManifest` (`heroes.dart:442-455`), minus everything a flight
/// needs and a measurement does not: no `overlay`, no `createRectTween`, no
/// `shuttleBuilder`, no `isDiverted`. Both rects are in their own route's coordinate
/// space, as `fromHeroLocation` / `toHeroLocation` are (`:514`, `:520`).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HeroFlightManifest {
    pub(crate) tag: HeroTag,
    /// `None` when neither route was animating; see [`FlightDirection::classify`].
    pub(crate) direction: Option<FlightDirection>,
    pub(crate) from_route: RouteId,
    pub(crate) to_route: RouteId,
    /// `fromHero`'s bounding box in `fromRoute`'s coordinate space (`:514`).
    pub(crate) from_rect: Rect,
    /// `toHero`'s bounding box in `toRoute`'s coordinate space (`:520`).
    pub(crate) to_rect: Rect,
    /// `manifest.isUserGestureTransition` (`heroes.dart:453`): started by
    /// `didStartUserGesture`, not a programmatic push/pop.
    pub(crate) is_user_gesture_transition: bool,
}

/// Watches a navigator, measures where hero flights land, and launches private flights.
///
/// Install with [`NavigatorHandle::add_observer`]. Holds no `GlobalKey`, reads no
/// element tree, and never touches the render tree from an observer callback — the
/// measurement happens in a post-frame callback, which is the only moment a route's
/// geometry is both committed and offstage.
#[derive(Default)]
pub struct HeroController {
    /// Flutter's `NavigatorObserver.navigator` (`navigator.dart:779`). `None` before
    /// attach and after detach, which is what makes a stale controller inert.
    navigator: Mutex<Option<NavigatorHandle>>,
    /// How many post-frame measurements have been *scheduled*. One per eligible
    /// push/pop, never one per observer callback.
    scheduled: Arc<AtomicUsize>,
    /// What those callbacks resolved, in order.
    measurements: Arc<Mutex<Vec<Measurement>>>,
    /// One per tag that both routes share and that measured to a finite rect.
    manifests: Arc<Mutex<Vec<HeroFlightManifest>>>,
    /// `HeroController._flights` (`heroes.dart:850`), one per tag in the air.
    flights: Arc<FlightManager>,
    /// `HeroController.createRectTween` (`heroes.dart:847`): the fallback rect-tween
    /// factory for heroes that set none of their own. `None` = linear.
    default_rect_factory: Option<RectTweenFactory>,
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
    /// `create_rect_tween` of its own. Flutter's `HeroController({this.createRectTween})`
    /// (`heroes.dart:840`); the `MaterialApp`-installed controller passes a
    /// `MaterialRectArcTween`. A per-`Hero` factory still overrides this (`:495`).
    #[must_use]
    pub fn with_rect_tween<F, A>(factory: F) -> Arc<Self>
    where
        F: Fn(Rect, Rect) -> A + 'static,
        A: Animatable<Rect> + Send + Sync + 'static,
    {
        Arc::new(Self {
            default_rect_factory: Some(Rc::new(move |begin, end| {
                Box::new(factory(begin, end)) as Box<dyn Animatable<Rect> + Send + Sync>
            })),
            ..Self::default()
        })
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
    pub(crate) fn flights(&self) -> &Arc<FlightManager> {
        &self.flights
    }

    /// Flutter's `_maybeStartHeroTransition` (`heroes.dart:892-976`): the
    /// eligibility test, then either the gesture-pop sync fast path or the
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
            return; // Detached. Flutter: `if (navigator == null) return;` (`:970`).
        };
        // `if (previousTopRoute == null) return;` (`:857-859`)
        let (Some(from), Some(to)) = (from, to) else {
            return;
        };
        // `if (toRoute == fromRoute || toRoute is! PageRoute || fromRoute is! PageRoute)`
        // (`:916-920`). `TransitionGroup::Page` already encodes `is PageRoute`,
        // because FLUI's routes name each other by id.
        if from == to || !navigator.is_page_route(from) || !navigator.is_page_route(to) {
            return;
        }

        // The two routes' `offstage` controls, which also carry the animation proxies
        // Flutter reads as `route.animation` (`routes.dart:1969`). A route that is not
        // a `ModalRoute` published none, and a disposed one withdrew it.
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

        // `:934-946` — a flight that has already arrived is not a flight. Note the
        // `null` arm falls through: no direction still measures.
        match direction {
            Some(FlightDirection::Pop) if from_animation.value() == 0.0 => return,
            Some(FlightDirection::Push) if to_animation.value() == 1.0 => return,
            _ => {}
        }

        // `:948-959` — the gesture-pop sync fast path: the destination route
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
                .and_then(|(subtree, owner)| owner.read().box_size(subtree.render_id))
                .is_some_and(Size::is_finite)
        {
            let pass = MeasurementPass {
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

        // Otherwise, delay measuring until the end of the next frame
        // (`:960-973`): `WidgetsBinding.instance.addPostFrameCallback(…)`
        // (`:968`). Acquired from a handle the navigator captured in
        // `init_state`, never from a frame phase.
        //
        // **Before the offstage flip, not after.** Flutter's `addPostFrameCallback`
        // cannot fail, so its setter runs first (`:967-968`); FLUI's capability is an
        // `Option` — absent on an unmounted navigator, or under a binding that
        // installs no post-frame handle. Flipping first and bailing here would strand
        // the destination offstage forever: nothing else ever calls
        // `set_offstage(false)`, because the only caller is the measurement we just
        // failed to schedule. Acquire, then mutate.
        let Some(post_frame) = navigator.post_frame_handle() else {
            return;
        };

        let measurements = Arc::clone(&self.measurements);
        let manifests = Arc::clone(&self.manifests);
        let flights = Arc::clone(&self.flights);
        let default_rect_factory = self.default_rect_factory.clone();
        let measured_destination = destination.clone();
        let schedule_result = post_frame.schedule_local(move |_timing| {
            let pass = MeasurementPass {
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
/// This is the prologue of `_startHeroTransition` (`heroes.dart:979-1060`): put the
/// destination back onstage, read the geometry the offstage frame committed, and match
/// the two routes' heroes by tag.
///
/// It runs in the **post-frame** phase of the frame the offstage flip dirtied, so
/// `box_size` and `transform_to` answer against committed layout.
/// Reading them from `did_change_top` instead would answer `None`, or worse, answer
/// with the *previous* frame's geometry.
///
/// A struct rather than a seven-argument function: it is the closure's payload, and
/// each field is one thing Flutter reads off `_HeroFlightManifest`.
struct MeasurementPass<'a> {
    navigator: &'a NavigatorHandle,
    source: &'a ModalHandle,
    destination: &'a ModalHandle,
    from: RouteId,
    to: RouteId,
    direction: Option<FlightDirection>,
    /// Whether this transition was started by `didStartUserGesture` rather
    /// than a programmatic push/pop — threaded into every
    /// [`HeroFlightManifest`] and used to filter heroes that did not opt in
    /// (`Hero._allHeroesFor`'s `inviteHero`, `heroes.dart:308-314`).
    is_user_gesture_transition: bool,
    flights: &'a Arc<FlightManager>,
    /// The controller-level `create_rect_tween` fallback (`heroes.dart:847`), used for a
    /// hero that set none of its own.
    default_rect_factory: &'a Option<RectTweenFactory>,
}

impl MeasurementPass<'_> {
    fn run(
        &self,
        measurements: &Mutex<Vec<Measurement>>,
        manifests: &Mutex<Vec<HeroFlightManifest>>,
    ) {
        // `if (fromRoute.navigator == null || toRoute.navigator == null) return;`
        // (`:969-972`) — the navigator may have left the tree while we waited.
        if !self.navigator.is_mounted() {
            return;
        }

        // Read before `start_transition` restores offstage: this is the value the
        // frame under measurement actually laid out with.
        let to_animation_while_offstage = self.destination.primary_animation().value();

        let to_subtree = self.navigator.route_subtree(self.to);
        let owner = self.navigator.render_tree();

        let (to_size, to_transform) = match (to_subtree, owner) {
            (Some(subtree), Some(owner)) => {
                let owner = owner.read();
                let transform = owner
                    .root_id()
                    .and_then(|root| owner.transform_to(subtree.render_id, root));
                (owner.box_size(subtree.render_id), transform)
            }
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

    /// `_startHeroTransition`'s manifest-collection-and-launch core
    /// (`heroes.dart:979-1060`, from `to.offstage = false` onward) — shared by
    /// [`run`](Self::run) (the offstage-then-post-frame path) and
    /// `HeroController::maybe_start`'s gesture-pop sync fast path, which calls
    /// this directly with no offstage flip: the destination route is already
    /// laid out, so there is nothing to restore.
    fn start_transition(&self) -> Vec<HeroFlightManifest> {
        // `to.offstage = false;` (`:987`) — a no-op when the sync fast path
        // never flipped it. Geometry stays committed until the next layout, so
        // measuring after this is safe either way.
        self.destination.set_offstage(false);

        // Retired flights are dropped here, outside every animation listener — see
        // `FlightManager`'s type docs for why that matters.
        self.flights.drain_retired();

        // Hand the flight manager the capability it needs to drain finished flights at
        // end-of-frame, before any of them can finish. Same handle the pass itself was
        // scheduled through, so it targets the binding's scheduler.
        self.flights
            .set_post_frame(self.navigator.post_frame_handle());

        let started = self.collect_manifests();
        for (manifest, from_hero, to_hero) in &started {
            self.launch(manifest, from_hero, to_hero);
        }
        started.into_iter().map(|(manifest, ..)| manifest).collect()
    }

    /// `_HeroFlight(_handleFlightEnded)..start(manifest)` (`heroes.dart:1054`).
    ///
    /// A manifest with **no direction** never flies: Flutter's `flightType == null`
    /// arm builds no manifest at all (`:1030`). The measurement is still recorded,
    /// because a manifest is measurement data, independent of whether a flight launches.
    fn launch(&self, manifest: &HeroFlightManifest, from_hero: &HeroHandle, to_hero: &HeroHandle) {
        let Some(direction) = manifest.direction else {
            return;
        };
        let Some(to_subtree) = self.navigator.route_subtree(self.to) else {
            return;
        };

        // `manifest.animation` (`:472-491`): the destination route's primary animation
        // drives a push, the source route's drives a pop. The `ModalRoute` proxy, not
        // the raw controller — so an offstage route reads `1.0`, as it must.
        let (route_animation, curve_hero) = match direction {
            FlightDirection::Push => (self.destination.primary_animation(), to_hero),
            FlightDirection::Pop => (self.source.primary_animation(), from_hero),
        };

        // Wrapped in a `CurvedAnimation` on the driving hero's `curve` — the
        // destination's for a push, the source's for a pop (`:474-485`). The reverse
        // curve defaults to the forward curve flipped (`:480`, `:484`), and a manifest
        // that diverts an airborne flight carries none (`isDiverted ? null :
        // reverseCurve`, `:490`).
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
        let animation: Arc<dyn Animation<f32>> = Arc::new(curved);

        // `toHero.widget.createRectTween ?? this.createRectTween` (`heroes.dart:495`):
        // the destination hero's factory wins, then the controller's default, then linear.
        let rect_factory = to_hero
            .rect_factory()
            .or_else(|| self.default_rect_factory.clone());

        // `toHero.widget.flightShuttleBuilder ?? fromHero.widget.flightShuttleBuilder`
        // (`heroes.dart:1040-1041`): the destination's wins, then the source's, then the
        // default shuttle.
        let shuttle_builder = to_hero
            .shuttle_builder()
            .or_else(|| from_hero.shuttle_builder());

        self.flights.start(
            manifest,
            FlightPlan {
                direction,
                from_hero: from_hero.clone(),
                to_hero: to_hero.clone(),
                to_route_subtree: to_subtree.render_id,
                overlay: self.navigator.overlay().clone(),
                animation,
                rect_factory,
                shuttle_builder,
                is_user_gesture_transition: self.is_user_gesture_transition,
                gesture_signal: self.navigator.user_gesture_signal(),
            },
        );
    }

    /// `Hero._allHeroesFor`'s `inviteHero`/else-branch (`heroes.dart:308-314`):
    /// during a gesture-driven transition, a hero that did not opt into
    /// [`transition_on_user_gestures`](super::hero::Hero::transition_on_user_gestures)
    /// is excluded from the match — but not silently. It is told to drop any
    /// placeholder a *prior* flight left it in, so a gesture transition
    /// interleaved with a programmatic one never leaves a non-opted-in hero
    /// hidden. A no-op for a programmatic transition
    /// (`is_user_gesture_transition == false`), where every hero participates
    /// unconditionally, exactly as `!isUserGestureTransition` short-circuits
    /// the oracle's own check.
    fn filter_for_gesture(
        &self,
        heroes: HashMap<HeroTag, HeroHandle>,
    ) -> HashMap<HeroTag, HeroHandle> {
        if !self.is_user_gesture_transition {
            return heroes;
        }
        heroes
            .into_iter()
            .filter_map(|(tag, hero)| {
                if hero.transition_on_user_gestures() {
                    Some((tag, hero))
                } else {
                    hero.end_flight(false);
                    None
                }
            })
            .collect()
    }

    /// `_startHeroTransition`'s matching loop (`heroes.dart:1014-1060`), reduced to
    /// what a measurement needs: every tag both routes carry, with both bounding boxes
    /// resolved in their own route's coordinate space.
    ///
    /// Flutter walks `fromHeroes.entries` and looks each tag up in `toHeroes`; a tag
    /// on only one side has no flight (`:1044-1046` — `toHero == null` ⇒ `endFlight`).
    /// Nothing here depends on iteration order. Both sides are gathered through
    /// [`ModalHandle::all_heroes`], so a tag registered inside a nested `Navigator`'s
    /// current `PageRoute` matches exactly as one registered directly on `from`/`to`
    /// — Flutter's nested-`Navigator` branch of `_allHeroesFor` (`heroes.dart:317-333`).
    fn collect_manifests(&self) -> Vec<(HeroFlightManifest, HeroHandle, HeroHandle)> {
        let Some(from_subtree) = self.navigator.route_subtree(self.from) else {
            return Vec::new();
        };
        let Some(to_subtree) = self.navigator.route_subtree(self.to) else {
            return Vec::new();
        };

        let from_heroes = self.filter_for_gesture(self.source.all_heroes());
        let to_heroes = self.filter_for_gesture(self.destination.all_heroes());

        let mut manifests = Vec::new();
        for (tag, from_hero) in &from_heroes {
            let Some(to_hero) = to_heroes.get(tag) else {
                continue; // A tag on only one route is not a flight.
            };

            // `_allHeroesFor` never visits a hero under a disabled `HeroMode`
            // (`heroes.dart:335-337`), so a disabled hero is missing from its route's
            // map — and a tag missing on either side is not a flight (`:1044-1046`).
            // FLUI registers the hero and skips it here instead.
            if !from_hero.hero_mode_enabled() || !to_hero.hero_mode_enabled() {
                continue;
            }

            let (Some(from_rect), Some(to_rect)) = (
                from_hero.bounding_box_in(from_subtree.render_id),
                to_hero.bounding_box_in(to_subtree.render_id),
            ) else {
                // Unmounted, or never laid out. Flutter asserts `box.hasSize` here and
                // crashes; a hero on a route that never built simply does not fly.
                tracing::debug!(?tag, "hero is not measurable; no flight");
                continue;
            };

            if !is_valid_flight(from_rect, to_rect) {
                tracing::warn!(?tag, "hero flight rect is not finite; skipping");
                continue;
            }

            manifests.push((
                HeroFlightManifest {
                    tag: tag.clone(),
                    direction: self.direction,
                    from_route: self.from,
                    to_route: self.to,
                    from_rect,
                    to_rect,
                    is_user_gesture_transition: self.is_user_gesture_transition,
                },
                from_hero.clone(),
                to_hero.clone(),
            ));
        }
        manifests
    }
}

impl NavigatorObserver for HeroController {
    /// This observer drives hero flights; see [`NavigatorObserver::observes_hero_flights`].
    fn observes_hero_flights(&self) -> bool {
        true
    }

    /// `NavigatorObserver._navigators[this] = navigator` (`navigator.dart:3836`).
    ///
    /// **A controller cannot be shared by two mounted navigators** (Flutter's "can
    /// not be shared", `:4010-4027`). If it is already attached
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

    /// `… = null` (`:4108`). A controller that keeps observing a detached navigator
    /// would schedule against a dead binding.
    fn did_detach(&self) {
        *self.navigator.lock() = None;
    }

    /// `HeroController.didChangeTop` (`heroes.dart:853-869`) — the **only** route
    /// callback it overrides.
    ///
    /// `didPush` / `didPop` are the wrong hook: they fire for routes that never
    /// become the top one (a `pushAndRemoveUntil` beneath the current top), and they
    /// do not fire when a route becomes top by having its cover popped. Flutter's
    /// `assert(topRoute.isCurrent)` says as much.
    fn did_change_top(&self, top: RouteId, previous_top: Option<RouteId>) {
        // Flutter asserts `topRoute.isCurrent` here (`heroes.dart:855`). FLUI cannot:
        // notifications are delivered *outside* the history lock and permit an
        // observer to mutate the stack from a callback, so a re-entrant push can leave
        // `top` transiently not-current by the time this fires. The flight path is
        // guarded downstream anyway (`route_peer`/`route_modal` return `None` for a
        // superseded route), so a stale top simply measures nothing.
        //
        // `didChangeTop`'s own guard (`heroes.dart:861`): "Don't trigger another
        // flight when a pop is committed as a user gesture back swipe is snapped."
        // A finger-driven pop still fires `did_change_top` (the swipe's own commit
        // pops the route), but a user gesture stays "in progress" until the
        // settling controller reports its final status (see `back_gesture.rs`), so
        // this still observes `user_gesture_in_progress() == true` at that instant
        // and suppresses the flight — this is not a substitute for
        // `did_start_user_gesture` below (which already ran its own flight, if any,
        // the instant the drag began); it is Flutter's own belt-and-braces guard
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

    /// `HeroController.didStartUserGesture` (`heroes.dart:871-879`): the
    /// instant a gesture (e.g. an edge swipe-back) begins, run the same
    /// eligibility-and-launch path `did_change_top` would, but pre-emptively
    /// and always classified as a pop (`FlightDirection::classify`'s
    /// `is_user_gesture_transition` arm) — this is what gives the very first
    /// drag frame a flight already in progress instead of waiting for a route
    /// change to commit.
    ///
    /// Note the argument order: `fromRoute: route, toRoute: previousRoute`
    /// (`:874-878`) — the route being dragged away from is `from`, the one it
    /// would reveal is `to`, exactly `maybe_start`'s own `(from, to)` shape.
    fn did_start_user_gesture(&self, route: RouteId, previous: Option<RouteId>) {
        self.maybe_start(Some(route), previous, true);
    }

    /// `HeroController.didStopUserGesture` (`heroes.dart:881-907`).
    ///
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
