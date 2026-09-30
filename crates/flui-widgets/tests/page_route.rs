//! Tests for [`PageRoute`] and [`PopupRoute`].
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/lib/src/widgets/pages.dart:50-61` (`PageRoute.opaque`,
//! `canTransitionTo`, `canTransitionFrom`), `.../widgets/routes.dart:2391-2394`
//! (`PopupRoute.opaque`, `maintainState`), `:293-321` (`_handleStatusChanged`),
//! `:422-496` (`_updateSecondaryAnimation`). Expected values are read from the
//! reference, not from running this code.
//!
//! These drive the animation by hand, through the temporary test-access probe
//! `RouteProbe::transition_handle()` (ADR-0083 §4).

use flui_animation::{Animation, AnimationStatus};
use flui_view::prelude::*;
use flui_view::{BoxedView, BuildContext};
use flui_widgets::__test_access::{
    NavigatorProbe as _, OverlayEntryProbe as _, RouteProbe as _, TransitionHandle,
};
use flui_widgets::SizedBox;
use flui_widgets::navigator::{
    Navigator, NavigatorHandle, PageRoute, RouteAnimation, RouteId, SimpleRoute,
};

use crate::common::harness::{Harness, mount};

fn leaf(_ctx: &dyn BuildContext, _a: &RouteAnimation, _s: &RouteAnimation) -> BoxedView {
    SizedBox::new(10.0, 10.0).into_view().boxed()
}

fn plain_page() -> SimpleRoute<i32> {
    SimpleRoute::new(|_ctx| SizedBox::new(10.0, 10.0).into_view().boxed())
}

/// A navigator with one non-animated route seeded, mounted and settled.
fn navigator_with_seed() -> (NavigatorHandle, Harness, RouteId) {
    let handle = NavigatorHandle::new();
    handle.seed_initial(plain_page());
    let harness = mount(Navigator::new(handle.clone()));
    let bottom = handle.route_ids()[0];
    (handle, harness, bottom)
}

/// Run an entrance transition to completion, then settle the owner status bridge
/// and the overlay rebuild it schedules. `set_value(1.0)` fires `Completed`,
/// which is queued by the animation listener and drained from owner-local
/// `ModalScope` build; the resulting `OverlayEntry.opaque` write rebuilds the
/// overlay on the following tick.
fn complete_entrance(transition: &TransitionHandle, harness: &mut Harness) {
    let controller = transition.controller().expect("install created it");
    controller.set_value(1.0);
    assert_eq!(controller.status(), AnimationStatus::Completed);
    harness.tick();
    harness.tick();
}

// ============================================================================
// opaque — pages.dart:50, routes.dart:2391
// ============================================================================

/// `PageRoute.opaque => true` (`pages.dart:50`): once the entrance transition
/// completes, the route below is dropped from the widget tree.
pub(crate) fn page_route_occludes_the_route_below_once_its_transition_completes() {
    let (navigator, mut harness, bottom) = navigator_with_seed();
    let bottom_entry = navigator
        .entry_of(bottom)
        .expect("seeded route has an entry");

    let route = PageRoute::<i32>::new(leaf);
    let transition = route.transition_handle();
    let _result = navigator.push(route);
    harness.tick();

    let top = *navigator.route_ids().last().expect("pushed");
    assert!(
        !navigator.entry_of(top).expect("entry").opaque(),
        "a route mid-transition never occludes"
    );
    assert!(bottom_entry.is_mounted());

    complete_entrance(&transition, &mut harness);

    assert!(navigator.entry_of(top).expect("entry").opaque());
    assert!(
        !bottom_entry.is_mounted(),
        "the covered route has no maintain_state, so it left the tree"
    );
}

// ============================================================================
// maintainState — routes.dart:1893, :2230, :2394
// ============================================================================

// ============================================================================
// secondaryAnimation — routes.dart:422-496, pages.dart:58-61
// ============================================================================

/// Pushing a `PageRoute` over a `PageRoute` drives the lower route's
/// `secondaryAnimation` from the upper route's primary animation
/// (`routes.dart:429-443`). Popping it re-points the proxy at the popped route,
/// so the lower page animates back in as the upper reverses away (`:393-402`).
pub(crate) fn secondary_animation_runs_on_the_previous_page_route_when_pushing_and_popping() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let lower = PageRoute::<i32>::new(leaf);
    let lower_transition = lower.transition_handle();
    let _lower = navigator.push(lower);
    harness.tick();
    complete_entrance(&lower_transition, &mut harness);

    assert!(
        lower_transition.secondary_is_dismissed(),
        "no route above: the proxy rests at kAlwaysDismissedAnimation"
    );

    let upper = PageRoute::<i32>::new(leaf);
    let upper_transition = upper.transition_handle();
    let _upper = navigator.push(upper);
    harness.tick();

    let secondary = lower_transition.secondary_animation();
    let upper_controller = upper_transition.controller().expect("installed");

    upper_controller.set_value(0.4);
    assert!(
        (secondary.value() - 0.4).abs() < 1e-6,
        "the lower page's secondaryAnimation tracks the upper page's animation, got {}",
        secondary.value()
    );

    upper_controller.set_value(1.0);
    harness.tick();
    assert!((secondary.value() - 1.0).abs() < 1e-6);

    assert!(navigator.pop());
    harness.tick();
    upper_controller.set_value(0.25);
    assert!(
        (secondary.value() - 0.25).abs() < 1e-6,
        "popping drives the secondary animation backwards, got {}",
        secondary.value()
    );
}

// ============================================================================
// pop — routes.dart:84-94, :177, :308-317
// ============================================================================

// ============================================================================
// barrier — routes.dart:2273-2330
// ============================================================================

// ============================================================================
// buildPage / buildTransitions — routes.dart:1229-1240, :1656
// ============================================================================

// ============================================================================
// back_gesture — the swipe-back detector substrate (back_gesture.rs)
// ============================================================================

/// A real, hit-tested horizontal drag through the mounted tree must move the
/// controller's value by `delta / route_width` — the harness's fixed 800px
/// screen, which the route's page fills
/// (`Stack(fit: expand)`) — never by `delta / BACK_GESTURE_WIDTH` (20px).
/// This is exactly the path that would have caught
/// `BackGestureRuntime::normalized_width` never reading the route's real
/// laid-out size and silently normalizing against the 20px hit strip
/// instead.
///
/// Every dispatched position stays inside the 20px-wide, full-height edge
/// strip on purpose: the harness re-hit-tests at each call, so a position
/// outside the strip's own bounds would
/// simply stop reaching the detector's `Listener` — the same constraint
/// `gesture_detector_recognizes_a_pan_and_suppresses_the_tap`
/// (`tests/gesture_detector.rs`) documents for that harness. The
/// recognizer's own delta reporting (`DragGestureRecognizer::handle_move`)
/// is a clean *incremental* delta since the last update once the run has
/// started, not "total move minus slop" — so a sub-pixel post-slop move is
/// still a clean, exact signal, not a fragile one.
///
/// Red-check: put back `width: Cell::new(BACK_GESTURE_WIDTH)` with no
/// refresh — the observed value drop becomes ~40x larger (18.9/20 instead
/// of 18.9/800) and the exact expectation fails.
pub(crate) fn back_gesture_edge_drag_normalizes_against_the_routes_real_width_not_the_hit_strip() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let route = PageRoute::<i32>::new(leaf).back_gesture(true);
    let transition = route.transition_handle();
    let _pushed = harness.enter_owner_scope(|| navigator.push(route));
    complete_entrance(&transition, &mut harness);
    let controller = transition.controller().expect("installed");
    assert_eq!(controller.value(), 1.0);

    // Down at x=1 (inside [0, 20)). This is the arena's lone recognizer, so
    // Flutter's deferred default accepts it after Down; both subsequent moves
    // are updates. The total logical movement is therefore 18.9px, and both
    // positions stay inside the 20px strip.
    harness.dispatch_pointer_down(1.0, 300.0);
    harness.dispatch_pointer_move(19.5, 300.0);
    harness.dispatch_pointer_move(19.9, 300.0);

    let dropped = 1.0 - controller.value();
    assert!(
        dropped > 0.0,
        "the drag must have moved the controller at all through real hit-testing \
         — got value={}",
        controller.value()
    );
    let expected = (19.9 - 1.0) / 800.0;
    assert!(
        (dropped - expected).abs() < 1e-4,
        "18.9px over an 800px-wide route should drop by {expected}; got \
         {dropped}. Normalizing against the 20px hit strip would be ~0.945"
    );

    harness.dispatch_pointer_up(19.9, 300.0);
}
