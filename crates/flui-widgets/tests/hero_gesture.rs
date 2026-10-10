//! User-gesture hero flights — `Hero.transitionOnUserGestures`.
//!
//! Every test here drives a real [`BackGestureController`] against a real
//! [`HeroController`], exactly as an edge swipe-back would, and observes the
//! private flight/hero seams directly — the crate-internal counterpart to
//! `tests/hero_public.rs`'s render-tree observation, needed here because
//! [`BackGestureController`] itself is `pub(crate)`.
//!
//! # Covered behavior
//!
//! The controller's user-gesture start and stop handling, the valid-size fast
//! path when a hero transition starts, inviting a hero into a flight, and the
//! flight's reaction to animation updates.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::AnimationController;
use flui_foundation::ValueKey;
use flui_view::ViewExt;
use flui_view::prelude::*;

use flui_widgets::__test_access::{
    BackGestureController, HeroControllerProbe as _, HeroTag, NavigatorProbe as _, RouteProbe as _,
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
    // the flight that push launched, so the explicit `controller` starts clean.
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

// ============================================================================
// 2. One-end-only opt-in: no flight
// ============================================================================

// ============================================================================
// 3. Non-opted hero un-hidden (the endFlight else-branch)
// ============================================================================

pub(crate) fn an_excluding_gesture_restores_pending_programmatic_placeholders() {
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(hero_page(false, 40.0, 24.0));
    let mut harness = mount_navigator(&navigator);
    let controller = install(&navigator);
    let destination = navigator.current().expect("the initial Hero page mounted");
    let route = hero_page(false, 30.0, 18.0).back_gesture(true);
    let transition = route.transition_handle();
    let _push = harness.enter_owner_scope(|| navigator.push(route));
    harness.tick();
    let source = navigator.current().expect("the covering Hero page mounted");
    let from = navigator
        .route_modal(source)
        .expect("the source modal")
        .all_heroes()
        .remove(&hero_tag())
        .expect("the source Hero mounted");
    let to = navigator
        .route_modal(destination)
        .expect("the destination modal")
        .all_heroes()
        .remove(&hero_tag())
        .expect("the destination Hero mounted");
    assert_eq!(
        controller.flights().len(),
        1,
        "the programmatic flight started"
    );
    assert!(from.placeholder_size().is_some() && to.placeholder_size().is_some());
    let animation = transition
        .controller()
        .expect("the route owns its animation");
    animation.set_value(1.0);
    assert_eq!(
        controller.flights().len(),
        1,
        "the shuttle has not drained completion yet"
    );

    let gesture = BackGestureController::new(navigator, source, animation);
    for hero in [&from, &to] {
        assert_eq!(
            hero.placeholder_size(),
            None,
            "gesture exclusion restores the preceding programmatic placeholder"
        );
    }
    harness.tick();
    for hero in [&from, &to] {
        assert_eq!(
            hero.placeholder_size(),
            None,
            "deferred completion cannot refreeze an excluded Hero"
        );
    }
    assert_eq!(controller.flights().len(), 0);
    assert!(
        !gesture.drag_end(0.0),
        "the already completed route needs no settling run"
    );
    harness.tick();
}

// ============================================================================
// 4. Mid-drag return to zero: deferral, not teardown
// ============================================================================

// ============================================================================
// 5. Cancel-release: flight returns, page state preserved
// ============================================================================

// ============================================================================
// 6. Complete-release: flight lands at the to-hero
// ============================================================================

/// A completed gesture (release with no fling, past the halfway point toward
/// zero) pops through to the destination route — synchronously, since the
/// stack mutation itself does not wait for the release animation. Once the
/// release's own 350ms pacing run actually settles (driven here by
/// `AnimationController::tick_at`) and
/// the navigator reports the gesture stopped, the parked terminal status
/// replays and the flight lands. The source stays frozen until its route
/// unmounts; disposal withdraws that placeholder from any retained handle.
/// The destination's real child returns at its committed size.
pub(crate) fn complete_release_pops_to_the_destination_route_and_the_flight_lands() {
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
    from_controller.tick_at(std::time::Duration::from_secs_f64(0.35));
    if still_settling {
        navigator.did_stop_user_gesture();
    }
    assert!(
        from_hero.placeholder_size().is_some(),
        "the source remains frozen until its route unmounts"
    );
    // The parked terminal status was just replayed (written + the shuttle
    // woken); this tick is what actually drains it and calls `finish`.
    harness.tick();

    assert!(
        controller.flights().get(&hero_tag()).is_none(),
        "the flight lands once the release genuinely settles"
    );
    assert_eq!(
        from_hero.render_id(),
        None,
        "the popped source cannot reappear"
    );
    assert_eq!(
        from_hero.placeholder_size(),
        None,
        "the unmounted handle has no live placeholder"
    );
    let destination = navigator
        .route_modal(to)
        .expect("the destination route remains mounted")
        .all_heroes()
        .get(&hero_tag())
        .cloned()
        .expect("the destination hero remains registered");
    assert_eq!(destination.placeholder_size(), None);
    let render = destination
        .render_id()
        .expect("the real destination child is attached");
    assert_eq!(
        harness
            .pipeline_owner()
            .with(|owner| owner.box_size(render)),
        Some(flui_foundation::geometry::Size::new(40.0, 24.0))
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
