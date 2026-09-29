//! User-gesture hero flights — `Hero.transitionOnUserGestures`.
//!
//! Every test here drives a real [`BackGestureController`] against a real
//! [`HeroController`], exactly as an edge swipe-back would, and observes the
//! private flight/hero seams directly — the crate-internal counterpart to
//! `tests/hero_public.rs`'s render-tree observation, needed here because
//! [`BackGestureController`] itself is `pub(crate)`.
//!
//! # Oracle
//!
//! `.flutter/packages/flutter/lib/src/widgets/heroes.dart` (3.44.0):
//! `HeroController.didStartUserGesture` / `didStopUserGesture` (`:871-907`),
//! `_maybeStartHeroTransition`'s `hasValidSize` fast path (`:948-959`),
//! `Hero._allHeroesFor`'s `inviteHero` (`:308-314`), and
//! `_HeroFlight._handleAnimationUpdate` (`:622-650`).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use flui_animation::AnimationController;
use flui_foundation::ValueKey;
use flui_view::ViewExt;
use flui_view::prelude::*;

use flui_widgets::__test_access::{
    BackGestureController, HeroControllerProbe as _, HeroTag, NavigatorProbe as _,
    OverlayProbe as _, RouteProbe as _,
};
use flui_widgets::navigator::{
    Hero, HeroController, Navigator, NavigatorHandle, NavigatorObserver, PageRoute, RouteId,
    SimpleRoute,
};
use flui_widgets::{Center, SizedBox};

use crate::common::harness::{Harness, mount};

const TRANSITION: Duration = Duration::from_millis(300);

fn hero_tag() -> HeroTag {
    HeroTag::new(ValueKey::new("shared"))
}

/// A `PageRoute` whose page centres one `Hero` tagged `"shared"`, sized
/// `width`x`height` so two pages never accidentally share a bounding rect.
fn hero_page(opt_in: bool, width: f64, height: f64) -> PageRoute<i32> {
    PageRoute::<i32>::new(move |_ctx, _p, _s| {
        Center::new()
            .child(
                Hero::new(ValueKey::new("shared"), SizedBox::new(width, height))
                    .transition_on_user_gestures(opt_in),
            )
            .into_view()
            .boxed()
    })
    .transition_duration(TRANSITION)
}

fn install(navigator: &NavigatorHandle) -> Arc<HeroController> {
    let controller = HeroController::new();
    navigator.add_observer(Arc::clone(&controller) as Arc<dyn NavigatorObserver>);
    controller
}

#[derive(Clone)]
struct Root {
    navigator: NavigatorHandle,
}

impl View for Root {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for Root {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        Navigator::new(self.navigator.clone())
    }
}

fn mount_navigator(navigator: &NavigatorHandle) -> Harness {
    mount(Root {
        navigator: navigator.clone(),
    })
}

/// A mounted navigator with a base (non-hero) route, a hero page the gesture
/// will reveal (`to`), and a second hero page pushed on top of it (`from`) —
/// the one a [`BackGestureController`] drags. Both hero pages share the tag
/// `"shared"` but differ in size, so a flight's `begin`/`end` rects are never
/// accidentally equal.
///
/// The `from` route's own transition controller is left at `set_value(1.0)`
/// — "fully on top, not yet popped" — the resting state a real edge-swipe
/// starts from; a [`BackGestureController`] then drags it down from there.
fn gesture_fixture_with(
    to_opt_in: bool,
    from_opt_in: bool,
    to_maintain_state: bool,
) -> (
    NavigatorHandle,
    Harness,
    Arc<HeroController>,
    RouteId,
    RouteId,
    AnimationController,
) {
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(1.0, 1.0).into_view().boxed()
    }));
    // No EXPLICIT controller yet — but `mount_navigator` already attached the
    // Navigator's own auto-installed hero observer (`did_change_top` does not
    // consult `transition_on_user_gestures` at all), so both pushes below are
    // already live to a `HeroController` by the time they run. Confirmed via
    // direct instrumentation: pushing `from_route` (both hero pages share the
    // tag `"shared"`) launches a real programmatic flight on the AUTO
    // observer's own `FlightManager` — a store this fixture's `controller`
    // never reads, so it cannot contaminate `controller.flights()`. `install`
    // below both swaps which observer receives FUTURE notifications and —
    // since `did_detach` sweeps the auto observer's in-flight flights — retires
    // the flight that push launched, so the explicit `controller` starts clean
    // (pinned by
    // `replacing_the_auto_hero_observer_retires_its_in_flight_flight`).
    let mut harness = mount_navigator(&navigator);

    let to_route = hero_page(to_opt_in, 40.0, 24.0).maintain_state(to_maintain_state);
    let _to_push = harness.enter_owner_scope(|| navigator.push(to_route));
    harness.tick();
    let to = navigator
        .current()
        .expect("the destination route is pushed");

    let from_route = hero_page(from_opt_in, 30.0, 18.0);
    let transition = from_route.transition_handle();
    let _from_push = harness.enter_owner_scope(|| navigator.push(from_route));
    harness.tick();
    let from = navigator.current().expect("the dragged route is pushed");

    // Attach the EXPLICIT controller only now — after both hero pages already
    // sit on the stack — so the gesture below is the first notification
    // *this* controller ever reacts to (it replaces the auto observer as of
    // this call; see the fixture's own doc for what that does and does not
    // clean up).
    let controller = install(&navigator);

    let from_controller = transition
        .controller()
        .expect("install() created the transition controller");
    from_controller.set_value(1.0);

    (navigator, harness, controller, to, from, from_controller)
}

fn gesture_fixture(
    to_opt_in: bool,
    from_opt_in: bool,
) -> (
    NavigatorHandle,
    Harness,
    Arc<HeroController>,
    RouteId,
    RouteId,
    AnimationController,
) {
    gesture_fixture_with(to_opt_in, from_opt_in, true)
}

// ============================================================================
// 1. Both ends opted in: synchronous start, shuttle tracks the drag
// ============================================================================

/// `_maybeStartHeroTransition`'s `hasValidSize` fast path (`heroes.dart:948-959`):
/// with the destination already laid out and `maintainState`, the flight starts
/// synchronously inside `did_start_user_gesture` — no frame needed — and the
/// shuttle's rect tracks the drag from that first instant.
///
/// Red-check: delete the sync fast-path branch from `HeroController::maybe_start`
/// — `controller.flights().get(&tag)` is `None` immediately after
/// `BackGestureController::new`, only appearing after a `harness.tick()`.
#[test]
fn gesture_pop_with_both_ends_opted_in_starts_synchronously_and_tracks_the_drag() {
    let (navigator, _harness, controller, _to, from, from_controller) = gesture_fixture(true, true);

    let gesture = BackGestureController::new(navigator, from, from_controller.clone());

    let flight = controller
        .flights()
        .get(&hero_tag())
        .expect("both ends opted in: the flight started synchronously, no frame needed");

    let begin = flight.begin_rect();
    let end = flight.target_rect();
    assert_ne!(
        begin, end,
        "the two hero pages differ in size, so begin and end must differ"
    );

    let before = flight.shuttle_rect();
    gesture.drag_update(0.5);
    let after = flight.shuttle_rect();
    assert_ne!(before, after, "the shuttle rect tracks the drag fraction");
}

// ============================================================================
// 2. One-end-only opt-in: no flight
// ============================================================================

// ============================================================================
// 3. Non-opted hero un-hidden (the endFlight else-branch)
// ============================================================================

// ============================================================================
// 4. Mid-drag return to zero: deferral, not teardown
// ============================================================================

// ============================================================================
// 5. Cancel-release: flight returns, page state preserved
// ============================================================================

/// A cancelled gesture (release with no fling, past the halfway point) stays
/// on the `from` page — and the *page's* state must survive the whole round
/// trip, not just its stack position: a real `StatefulView` `create_state`
/// counter, on a plain sibling of the hero (not the hero's own child), proves
/// the page was never torn down and rebuilt from scratch while the flight was
/// airborne and then aborted.
///
/// A sibling, deliberately, not the hero's own child: the hero *itself* is
/// this flight's `from_hero`, always classified `Pop`
/// (`FlightDirection::classify`), which Flutter starts with
/// `shouldIncludeChildInPlaceholder: false` (`heroes.dart:721-724`, ported by
/// `HeroFlight::start`'s `direction == Push` check) — so the hero's own child
/// is legitimately *not* preserved in place while airborne (same as
/// Flutter's own pop-source hero); pinning that non-preservation is not this
/// test's concern. What must hold regardless is that the surrounding page —
/// the route we stayed on — keeps everything else alive.
///
/// **The tick between the cancel and the assertion is load-bearing.** A
/// route rebuild is not synchronous with `drag_end`; checking `creations`
/// with no tick in between would pass even if the page were torn down and
/// rebuilt, because the rebuild that would prove it never runs.
///
/// Red-check: have `ModalScope` unconditionally discard and rebuild its page
/// subtree on every primary-animation notify instead of diffing it — after
/// the tick, `creations` reads more than `1`.
#[test]
fn cancel_release_preserves_the_from_pages_sibling_state() {
    #[derive(Clone)]
    struct Counter(Arc<AtomicUsize>);
    impl View for Counter {
        fn create_element(&self) -> flui_view::element::ElementKind {
            flui_view::element::ElementKind::stateful(self)
        }
    }
    impl StatefulView for Counter {
        type State = CounterState;
        fn create_state(&self) -> Self::State {
            self.0.fetch_add(1, Ordering::SeqCst);
            CounterState
        }
    }
    struct CounterState;
    impl ViewState<Counter> for CounterState {
        fn build(&self, _v: &Counter, _c: &dyn BuildContext) -> impl IntoView {
            SizedBox::new(1.0, 1.0)
        }
    }

    let creations = Arc::new(AtomicUsize::new(0));
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(1.0, 1.0).into_view().boxed()
    }));
    // No controller yet — see `gesture_fixture_with`'s doc for why: both hero
    // pages below share the tag `"shared"`, and an attached controller would
    // fly a real programmatic flight between them right here.
    let mut harness = mount_navigator(&navigator);

    let to_route = hero_page(true, 40.0, 24.0);
    let _to_push = harness.enter_owner_scope(|| navigator.push(to_route));
    harness.tick();

    let creations_for_page = Arc::clone(&creations);
    let from_route = PageRoute::<i32>::new(move |_ctx, _p, _s| {
        flui_widgets::Stack::new(vec![
            Hero::new(ValueKey::new("shared"), SizedBox::new(30.0, 18.0))
                .transition_on_user_gestures(true)
                .into_view()
                .boxed(),
            Counter(Arc::clone(&creations_for_page)).into_view().boxed(),
        ])
        .into_view()
        .boxed()
    })
    .transition_duration(TRANSITION);
    let transition = from_route.transition_handle();
    let _from_push = harness.enter_owner_scope(|| navigator.push(from_route));
    harness.tick();
    let from = navigator.current().expect("the from route is pushed");
    assert_eq!(creations.load(Ordering::SeqCst), 1, "built once");

    install(&navigator);

    let from_controller = transition
        .controller()
        .expect("install() created the transition controller");
    from_controller.set_value(1.0);

    let gesture = BackGestureController::new(navigator.clone(), from, from_controller.clone());
    gesture.drag_update(0.3); // Partway: value 0.7, past the halfway "stay" threshold.
    let _still_settling = gesture.drag_end(0.0); // No fling: value > 0.5 => cancel.
    // Let the cancel's return-to-normal shape actually build.
    harness.tick();

    assert_eq!(
        navigator.current(),
        Some(from),
        "a cancelled gesture stays on the from page"
    );
    assert_eq!(
        creations.load(Ordering::SeqCst),
        1,
        "the page's sibling state survived the cancelled gesture — no rebuild"
    );
}

// ============================================================================
// 6. Complete-release: flight lands at the to-hero
// ============================================================================

/// A completed gesture (release with no fling, past the halfway point toward
/// zero) pops through to the destination route — synchronously, since the
/// stack mutation itself does not wait for the release animation. Once the
/// release's own 350ms pacing run actually settles (driven here by
/// `AnimationController::tick_at`, matching `back_gesture.rs`'s own
/// `full_settle_after_release_reports_did_stop_and_clears_the_counter`) and
/// the navigator reports the gesture stopped, the parked terminal status
/// replays and the flight lands: `finish`'s `Completed` arm keeps the
/// (now-gone) from-hero's placeholder rather than clearing it
/// (`from_hero.end_flight(status.is_completed())`, `heroes.dart:614`).
#[test]
fn complete_release_pops_to_the_destination_route_and_the_flight_lands() {
    let (navigator, mut harness, controller, to, from, from_controller) =
        gesture_fixture(true, true);

    let from_modal = navigator
        .route_modal(from)
        .expect("a PageRoute publishes a ModalHandle");
    let from_hero = from_modal
        .all_heroes()
        .get(&hero_tag())
        .cloned()
        .expect("the from-hero registered with its route");

    let gesture = BackGestureController::new(navigator.clone(), from, from_controller.clone());
    gesture.drag_update(0.7); // value 0.3: drag_end's pop branch.
    let still_settling = gesture.drag_end(0.0);

    assert_eq!(
        navigator.current(),
        Some(to),
        "a completed gesture pops through to the destination route"
    );

    // Drive the release's own pacing run (350ms) to completion, then report
    // the gesture stopped — mirrors `BackGestureDetectorState::poll_settle`
    // once `!controller.is_animating()`.
    from_controller.tick_at(0.35);
    if still_settling {
        navigator.did_stop_user_gesture();
    }
    // The parked terminal status was just replayed (written + the shuttle
    // woken); this tick is what actually drains it and calls `finish`.
    harness.tick();

    assert!(
        controller.flights().get(&hero_tag()).is_none(),
        "the flight lands once the release genuinely settles"
    );
    assert!(
        from_hero.placeholder_size().is_some(),
        "a Completed pop keeps the from-hero's placeholder (heroes.dart:614) — \
         its route is gone, so its child must not reappear"
    );
}

// ============================================================================
// 7. Invalid destination size: falls back to the deferred path, no panic
// ============================================================================

// ============================================================================
// 8. Drag-never-moved release: did_stop dismisses the parked flight
// ============================================================================

// ============================================================================
// 9. Replacing the auto observer retires its in-flight flight
// ============================================================================

/// `HeroController.did_detach` retires the flights its controller still has in
/// the air — Flutter's `HeroController.dispose` sweeps `_flights`
/// (`heroes.dart:1112-1116`) when the controller is released, and here a
/// controller is released by *replacement* (`NavigatorHandle::add_observer`
/// takes the auto-default) while the navigator and its heroes stay alive.
///
/// Flutter never hits this because its `HeroController` is owned by the
/// navigator for its whole life; FLUI replaces the controller in place, so the
/// flight it launched must be torn down — its overlay entry removed and both
/// heroes' placeholders restored — rather than left to paint forever. Recorded
/// in `ARCHITECTURE.md` §18.
///
/// Red-check: delete the `self.flights.finish_all()` call from
/// `HeroController::did_detach` — the overlay count stays one entry high after
/// `install`, and both heroes keep their placeholders.
#[test]
fn replacing_the_auto_hero_observer_retires_its_in_flight_flight() {
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(1.0, 1.0).into_view().boxed()
    }));
    let mut harness = mount_navigator(&navigator);

    // First hero page — the base route carries no matching tag, so this push
    // (from a non-PageRoute) launches nothing.
    let to_route = hero_page(true, 40.0, 24.0);
    let _to_push = harness.enter_owner_scope(|| navigator.push(to_route));
    harness.tick();
    let to = navigator
        .current()
        .expect("the destination route is pushed");
    let pre_flight = navigator.overlay().len();

    // Second hero page shares the tag: the auto observer launches a real
    // programmatic flight during this settling tick, inserting one overlay entry.
    let from_route = hero_page(true, 30.0, 18.0);
    let _from_push = harness.enter_owner_scope(|| navigator.push(from_route));
    harness.tick();
    let from = navigator.current().expect("the dragged route is pushed");

    assert_eq!(
        navigator.overlay().len(),
        pre_flight + 2,
        "the second push added a route entry and the auto observer's flight entry"
    );

    // Replacing the auto observer must retire the flight it launched: the
    // overlay entry comes out (only the still-pushed routes remain) and both
    // heroes restore their children instead of a blank placeholder.
    let controller = install(&navigator);

    assert_eq!(
        navigator.overlay().len(),
        pre_flight + 1,
        "replacing the auto observer retired its flight and removed its overlay entry"
    );
    assert!(
        controller.flights().get(&hero_tag()).is_none(),
        "the replacement controller inherited no flight"
    );

    let to_hero = navigator
        .route_modal(to)
        .and_then(|m| m.all_heroes().get(&hero_tag()).cloned())
        .expect("the destination hero registered with its route");
    let from_hero = navigator
        .route_modal(from)
        .and_then(|m| m.all_heroes().get(&hero_tag()).cloned())
        .expect("the dragged hero registered with its route");
    assert!(
        to_hero.placeholder_size().is_none(),
        "the destination hero's placeholder is restored on retirement"
    );
    assert!(
        from_hero.placeholder_size().is_none(),
        "the dragged hero's placeholder is restored on retirement"
    );
}
