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

use flui_animation::{Animation, AnimationController, AnimationStatus};
use flui_foundation::ValueKey;
use flui_view::ViewExt;
use flui_view::prelude::*;

use super::back_gesture::BackGestureController;
use super::hero::{Hero, HeroTag};
use super::hero_controller::HeroController;
use super::navigator::{Navigator, NavigatorHandle};
use super::observer::NavigatorObserver;
use super::overlay_route::SimpleRoute;
use super::page_route::PageRoute;
use super::route::RouteId;
use crate::testing::harness::{Harness, mount};
use crate::{Center, SizedBox};

const TRANSITION: Duration = Duration::from_millis(300);

fn hero_tag() -> HeroTag {
    HeroTag::new(ValueKey::new("shared"))
}

/// A `PageRoute` whose page centres one `Hero` tagged `"shared"`, sized
/// `width`x`height` so two pages never accidentally share a bounding rect.
fn hero_page(opt_in: bool, width: f32, height: f32) -> PageRoute<i32> {
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

/// `Hero._allHeroesFor`'s `inviteHero` (`heroes.dart:308-314`): a pair flies
/// during a gesture transition only when **both** ends opt in.
///
/// Red-check: drop the `hero_mode_enabled`-style filter — `filter_for_gesture`
/// — from `MeasurementPass::collect_manifests` and this starts a flight anyway.
#[test]
fn one_end_only_opting_in_starts_no_flight() {
    let (navigator, _harness, controller, _to, from, from_controller) =
        gesture_fixture(true, false);

    let _gesture = BackGestureController::new(navigator, from, from_controller);

    assert!(
        controller.flights().get(&hero_tag()).is_none(),
        "the from-hero did not opt in, so no pair flies"
    );
}

// ============================================================================
// 3. Non-opted hero un-hidden (the endFlight else-branch)
// ============================================================================

/// The oracle's else-branch calls `endFlight` on a non-participating hero to
/// un-hide it if a prior flight left it hidden (`heroes.dart:311-314`).
///
/// Red-check: change `HeroController::filter_for_gesture` to plain
/// `.retain(...)` (drop the `hero.end_flight(false)` call in the rejected arm)
/// — the simulated prior flight's placeholder never clears.
#[test]
fn a_non_opted_heroes_placeholder_is_cleared_when_a_gesture_transition_starts() {
    let (navigator, _harness, _controller, _to, from, from_controller) =
        gesture_fixture(true, false);

    let from_modal = navigator
        .route_modal(from)
        .expect("a PageRoute publishes a ModalHandle");
    let hero = from_modal
        .all_heroes()
        .get(&hero_tag())
        .cloned()
        .expect("the hero registered with its route");
    // Simulate a prior (programmatic) flight that left this hero hidden.
    hero.start_flight(false);
    assert!(
        hero.placeholder_size().is_some(),
        "hidden by the simulated prior flight"
    );

    let _gesture = BackGestureController::new(navigator, from, from_controller);

    assert!(
        hero.placeholder_size().is_none(),
        "a gesture transition's per-hero filter un-hides a non-opted-in hero \
         instead of silently skipping it"
    );
}

// ============================================================================
// 4. Mid-drag return to zero: deferral, not teardown
// ============================================================================

/// `_HeroFlight._handleAnimationUpdate` (`heroes.dart:622-650`): a terminal
/// status update while the user gesture is in progress is parked, not applied
/// — dragging back to zero mid-gesture must not tear the flight down with the
/// finger still down, and dragging forward again must keep tracking it.
///
/// **The tick between each drag and its assertion is load-bearing, not
/// incidental.** A terminal status only ever tears the flight down once the
/// shuttle's own `build` drains `settled_status` (`FlightInner`'s data-plane
/// listener never calls `finish` itself); nothing about `drag_update` runs a
/// build synchronously. Asserting right after `drag_update`, with no tick,
/// would pass whether or not the deferral guard exists — there would be
/// nothing yet to drain either way, guard or no guard.
///
/// Red-check: delete the `gesture_signal.in_progress()` guard from the status
/// listener in `FlightManager::start` — after the tick following the full
/// swipe to zero, `flights().get(&tag)` returns `None`.
#[test]
fn a_full_swipe_to_zero_mid_gesture_does_not_tear_down_the_flight() {
    let (navigator, mut harness, controller, _to, from, from_controller) =
        gesture_fixture(true, true);

    let gesture = BackGestureController::new(navigator, from, from_controller.clone());
    assert!(
        controller.flights().get(&hero_tag()).is_some(),
        "flight started synchronously"
    );

    // Drag all the way: the from-route's controller hits exactly 0 — Dismissed
    // — while the finger is still down.
    gesture.drag_update(1.0);
    assert_eq!(from_controller.value(), 0.0);
    assert_eq!(from_controller.status(), AnimationStatus::Dismissed);
    // Give the shuttle the build that would drain a terminal status and call
    // `finish` if the deferral guard were missing.
    harness.tick();
    assert!(
        controller.flights().get(&hero_tag()).is_some(),
        "the flight must not tear down while the user gesture is still in progress"
    );

    // Drag forward again: still airborne, still tracked.
    gesture.drag_update(-0.6);
    assert!(from_controller.value() > 0.0);
    harness.tick();
    assert!(
        controller.flights().get(&hero_tag()).is_some(),
        "still airborne after dragging forward again"
    );
}

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
        crate::Stack::new(vec![
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

/// `_maybeStartHeroTransition`'s `hasValidSize` fast path requires
/// `toRoute.maintainState` (`heroes.dart:957`); without it, the transition
/// falls back to the ordinary offstage-then-post-frame path — same code path
/// a programmatic push/pop over the same destination would take — with no
/// panic anywhere.
///
/// A `maintainState == false` destination that is still covered when the
/// gesture starts has no mounted subtree to flip onstage in the first
/// place (`ModalRoute`'s own doc: "a covered modal with `maintain_state ==
/// false` is unmounted"; pinned directly by
/// `modal_covered_route_without_maintain_state_is_unmounted_and_loses_its_state`
/// in `modal_route_tests.rs`, once the covering transition completes), so
/// there is nothing to measure — the deferred path correctly measures
/// nothing, exactly as it would for a programmatic transition onto the same
/// unmeasurable destination
/// (`a_measurement_whose_navigator_vanished_before_the_frame_records_nothing`'s
/// sibling case). The behavior under test is "falls back, does not crash
/// trying, and finds nothing to fly" — not "recovers a flight from an
/// inherently unmeasurable route".
///
/// Red-check: drop the `destination.maintain_state()` conjunct from the sync
/// fast-path condition in `HeroController::maybe_start` — the flight starts
/// synchronously and the first assertion (`flights().get(...).is_none()`
/// before any tick) fails.
///
/// **Two episodes of one masked bug, not two different bugs.** "Nothing
/// measurable" was the right expectation from the start, but each time this
/// test was checked, something else was silently absorbing the covered
/// route's own unmount before this test's single `harness.tick()` could
/// observe it:
///
/// 1. (2026-08-05) The owner-local post-frame lane's old thread-local
///    "active lane" gate silently failed the deferred measurement's own
///    `schedule_local` call in this exact fixture (`gesture_fixture_with`
///    calls `BackGestureController::new` outside any `enter_owner_scope`),
///    so the measurement never ran at all and "nothing flies" passed
///    vacuously. Once `LocalPostFrameHandle` addressed its lane directly (no
///    "active lane" gate to fail), the measurement started genuinely
///    running — and found `to`'s subtree STILL mounted and laid out
///    (`route_subtree` returned `Some`, `box_size` reported the full
///    800x600 screen) despite `maintain_state(false)` and full coverage.
///    The test was flipped to expect that flight instead of asking why the
///    subtree was still there.
/// 2. (issue #1180) The covered route's own unmount is driven by a rebuild
///    that used to travel through `BuildOwner`'s external inbox, which
///    `drain_build_scope` drained only once, at the START of its heap loop —
///    so a rebuild a LATER build in that same drain schedules waited a
///    whole extra frame. This fixture's single settling `harness.tick()`
///    was one frame short of that wait, so the unmount had simply not
///    landed yet by the time this test measured — the identical masked-bug
///    shape as episode 1, moved from the post-frame lane to the inbox
///    deferral. `drain_build_scope` now absorbs that inbox at the top of
///    every heap pop, landing the unmount within the SAME tick.
///    Confirmed directly: `route_subtree(to)` is `Some` on the code before
///    this change and `None` after it, for the identical fixture state.
///
/// So the flight this test used to assert on was never a real one — it was
/// the same masked bug wearing a different mechanism. This is the original,
/// Flutter-correct expectation, restored.
#[test]
fn a_to_route_that_does_not_maintain_state_falls_back_to_the_deferred_path_without_panicking() {
    let (navigator, mut harness, controller, to, from, from_controller) =
        gesture_fixture_with(true, true, false);

    let _gesture = BackGestureController::new(navigator.clone(), from, from_controller);
    assert!(
        controller.flights().get(&hero_tag()).is_none(),
        "no maintainState on the destination: the sync fast path must not fire"
    );

    // The deferred (offstage-then-post-frame) path runs next — still no
    // panic, and the covered maintain_state(false) destination now has no
    // mounted subtree left to measure by the time it runs (issue #1180: the
    // covering transition's own unmount rebuild lands within this same
    // tick, not the next one).
    harness.tick();
    assert!(
        navigator.route_subtree(to).is_none(),
        "a covered maintain_state(false) destination must have no mounted \
         subtree by the time the deferred path measures it"
    );
    assert!(
        controller.flights().get(&hero_tag()).is_none(),
        "the deferred path measures nothing: the covered maintain_state(false) \
         destination is unmounted"
    );
}

// ============================================================================
// 8. Drag-never-moved release: did_stop dismisses the parked flight
// ============================================================================

/// `HeroController.didStopUserGesture`'s manual sweep (`heroes.dart:882-907`):
/// a gesture-driven pop flight whose proxy never left `Dismissed` (the drag
/// never moved) has no status transition to end it on its own — it must be
/// dismissed manually once the gesture genuinely stops.
///
/// Red-check: delete `HeroController::did_stop_user_gesture`'s
/// `finish_stalled_gesture_pops` call — the flight leaks forever, and
/// `flights().get(...)` still returns `Some` after this test's final step.
#[test]
fn a_never_moved_drag_is_dismissed_once_the_gesture_genuinely_stops() {
    let (navigator, _harness, controller, _to, from, from_controller) = gesture_fixture(true, true);

    let gesture = BackGestureController::new(navigator.clone(), from, from_controller.clone());
    assert!(
        controller.flights().get(&hero_tag()).is_some(),
        "flight started"
    );

    // Release without ever moving: `drag_end` at value 1.0, no fling — "stay".
    let still_settling = gesture.drag_end(0.0);
    if still_settling {
        // Mirrors what `BackGestureDetectorState::poll_settle` eventually
        // reports once the release run settles.
        navigator.did_stop_user_gesture();
    }

    assert!(
        controller.flights().get(&hero_tag()).is_none(),
        "did_stop_user_gesture must manually dismiss a flight whose drag never moved"
    );
}

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
