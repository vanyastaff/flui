//! ADR-0020 U5.4 tests for [`PageRoute`] and [`PopupRoute`].
//!
//! # Parity oracles
//!
//! `.flutter/packages/flutter/lib/src/widgets/pages.dart:50-61` (`PageRoute.opaque`,
//! `canTransitionTo`, `canTransitionFrom`), `.../widgets/routes.dart:2391-2394`
//! (`PopupRoute.opaque`, `maintainState`), `:293-321` (`_handleStatusChanged`),
//! `:422-496` (`_updateSecondaryAnimation`). Expected values are read from the
//! reference, not from running this code.
//!
//! These drive the animation by hand, through the `#[cfg(test)]`
//! `transition_handle()`. `tests/routes.rs` is the public counterpart: it pushes
//! the same routes through the prelude and drives a real `Vsync`.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use flui_animation::{Animation, AnimationStatus};
use flui_types::Color;
use flui_view::prelude::*;
use flui_view::{BoxedView, BuildContext};

use super::navigator::{Navigator, NavigatorHandle};
use super::overlay_route::{RouteAnimation, SimpleRoute};
use super::page_route::{PageRoute, PopupRoute};
use super::route::RouteId;
use super::transition_route::TransitionHandle;
use crate::SizedBox;
use crate::test_harness::{Harness, mount};

/// A leaf whose `create_state` is counted, so "was this subtree destroyed?" is
/// observable.
#[derive(Clone)]
struct Probe(Arc<AtomicUsize>);

impl View for Probe {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateful(self)
    }
}

impl StatefulView for Probe {
    type State = ProbeState;

    fn create_state(&self) -> Self::State {
        self.0.fetch_add(1, Ordering::Relaxed);
        ProbeState
    }
}

struct ProbeState;

impl ViewState<Probe> for ProbeState {
    fn build(&self, _view: &Probe, _ctx: &dyn BuildContext) -> impl IntoView {
        SizedBox::new(10.0, 10.0)
    }
}

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

/// Run an entrance transition to completion. `set_value(1.0)` fires `Completed`,
/// which is the status `_handleStatusChanged` acts on.
fn complete_entrance(transition: &TransitionHandle, harness: &mut Harness) {
    let controller = transition.controller().expect("install created it");
    controller.set_value(1.0);
    assert_eq!(controller.status(), AnimationStatus::Completed);
    harness.tick();
}

// ============================================================================
// opaque — pages.dart:50, routes.dart:2391
// ============================================================================

/// `PageRoute.opaque => true` (`pages.dart:50`): once the entrance transition
/// completes, the route below is dropped from the widget tree.
#[test]
fn page_route_occludes_the_route_below_once_its_transition_completes() {
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

/// `case forward: case reverse: overlayEntries.first.opaque = false`
/// (`routes.dart:303-305`). A page sliding away shows the one beneath it.
#[test]
fn page_route_clears_opaque_while_it_moves() {
    let (navigator, mut harness, bottom) = navigator_with_seed();
    let bottom_entry = navigator.entry_of(bottom).expect("entry");

    let route = PageRoute::<i32>::new(leaf);
    let transition = route.transition_handle();
    let _result = navigator.push(route);
    harness.tick();
    complete_entrance(&transition, &mut harness);

    let top = *navigator.route_ids().last().expect("pushed");
    assert!(navigator.entry_of(top).expect("entry").opaque());

    let controller = transition.controller().expect("installed");
    controller.reverse().expect("reverse from 1.0");
    harness.tick();

    assert_eq!(controller.status(), AnimationStatus::Reverse);
    assert!(!navigator.entry_of(top).expect("entry").opaque());
    assert!(bottom_entry.is_mounted(), "the page below shows through");
}

/// `PopupRoute.opaque => false` (`routes.dart:2391`): the page under a dialog
/// stays built and visible, even once the popup has fully arrived.
#[test]
fn popup_route_is_not_opaque_and_never_drops_the_route_below() {
    let (navigator, mut harness, bottom) = navigator_with_seed();
    let bottom_entry = navigator.entry_of(bottom).expect("entry");

    let route = PopupRoute::<i32>::new(leaf);
    let transition = route.transition_handle();
    let _result = navigator.push(route);
    harness.tick();
    complete_entrance(&transition, &mut harness);

    let top = *navigator.route_ids().last().expect("pushed");
    assert!(!navigator.entry_of(top).expect("entry").opaque());
    assert!(bottom_entry.is_mounted());
    assert_eq!(navigator.route_ids().len(), 2);
}

// ============================================================================
// maintainState — routes.dart:1893, :2230, :2394
// ============================================================================

/// `maintain_state == false` under an opaque `PageRoute`: the covered route is
/// unmounted and its state destroyed, then rebuilt fresh when uncovered.
#[test]
fn maintain_state_false_route_below_an_opaque_page_is_unmounted_and_rebuilt_fresh() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let creations = Arc::new(AtomicUsize::new(0));
    let covered = {
        let creations = Arc::clone(&creations);
        PageRoute::<i32>::new(move |_ctx, _a, _s| Probe(Arc::clone(&creations)).into_view().boxed())
            .maintain_state(false)
    };
    let covered_transition = covered.transition_handle();
    let _covered = navigator.push(covered);
    harness.tick();
    complete_entrance(&covered_transition, &mut harness);
    assert_eq!(creations.load(Ordering::Relaxed), 1);

    let coverer = PageRoute::<i32>::new(leaf);
    let coverer_transition = coverer.transition_handle();
    let _coverer = navigator.push(coverer);
    harness.tick();
    complete_entrance(&coverer_transition, &mut harness);

    let covered_id = navigator.route_ids()[1];
    assert!(
        !navigator.entry_of(covered_id).expect("entry").is_mounted(),
        "maintain_state == false: the covered page leaves the tree"
    );

    coverer_transition
        .controller()
        .expect("installed")
        .reverse()
        .expect("reverse from 1.0");
    harness.tick();

    assert!(navigator.entry_of(covered_id).expect("entry").is_mounted());
    assert_eq!(
        creations.load(Ordering::Relaxed),
        2,
        "the destroyed subtree is rebuilt with fresh state"
    );
}

/// `PageRoute`'s default `maintainState` is `true` (`pages.dart:101`), and so is
/// `PopupRoute`'s (`routes.dart:2394`). `install()` publishes it onto the entry.
#[test]
fn both_public_routes_publish_maintain_state_true_by_default() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let _page = navigator.push(PageRoute::<i32>::new(leaf));
    let _popup = navigator.push(PopupRoute::<i32>::new(leaf));
    harness.tick();

    for id in navigator.route_ids().into_iter().skip(1) {
        assert!(
            navigator.entry_of(id).expect("entry").maintain_state(),
            "route {id:?} must publish maintain_state = true at install"
        );
    }
}

// ============================================================================
// secondaryAnimation — routes.dart:422-496, pages.dart:58-61
// ============================================================================

/// Pushing a `PageRoute` over a `PageRoute` drives the lower route's
/// `secondaryAnimation` from the upper route's primary animation
/// (`routes.dart:429-443`). Popping it re-points the proxy at the popped route,
/// so the lower page animates back in as the upper reverses away (`:393-402`).
#[test]
fn secondary_animation_runs_on_the_previous_page_route_when_pushing_and_popping() {
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

/// `PageRoute.canTransitionTo(next) => next is PageRoute` (`pages.dart:58`).
///
/// A `PopupRoute` opening over a page must **not** slide the page away. FLUI
/// expresses the two symmetric predicates as a `TransitionGroup` on the peer.
#[test]
fn a_popup_over_a_page_route_drives_no_secondary_animation() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let page = PageRoute::<i32>::new(leaf);
    let page_transition = page.transition_handle();
    let _page = navigator.push(page);
    harness.tick();
    complete_entrance(&page_transition, &mut harness);

    let popup = PopupRoute::<i32>::new(leaf);
    let popup_transition = popup.transition_handle();
    let _popup = navigator.push(popup);
    harness.tick();

    assert!(
        page_transition.secondary_is_dismissed(),
        "a PageRoute coordinates only with another PageRoute"
    );

    popup_transition
        .controller()
        .expect("installed")
        .set_value(0.7);
    assert!(
        page_transition.secondary_animation().value().abs() < 1e-6,
        "the page must not move while a popup opens over it"
    );
}

/// The converse, for the same reason: a `PageRoute` pushed over a `PopupRoute`
/// fails `PageRoute.canTransitionFrom(popup)` (`pages.dart:61`).
#[test]
fn a_page_route_over_a_popup_drives_no_secondary_animation() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let popup = PopupRoute::<i32>::new(leaf);
    let popup_transition = popup.transition_handle();
    let _popup = navigator.push(popup);
    harness.tick();
    complete_entrance(&popup_transition, &mut harness);

    let page = PageRoute::<i32>::new(leaf);
    let page_transition = page.transition_handle();
    let _page = navigator.push(page);
    harness.tick();

    assert!(popup_transition.secondary_is_dismissed());
    page_transition
        .controller()
        .expect("installed")
        .set_value(0.7);
    assert!(popup_transition.secondary_animation().value().abs() < 1e-6);
}

/// Two popups *do* coordinate: `PopupRoute` inherits `TransitionRoute`'s
/// `canTransitionTo/From => true` (`routes.dart:536`, `:561`).
#[test]
fn two_popups_coordinate_their_transitions() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let lower = PopupRoute::<i32>::new(leaf);
    let lower_transition = lower.transition_handle();
    let _lower = navigator.push(lower);
    harness.tick();
    complete_entrance(&lower_transition, &mut harness);

    let upper = PopupRoute::<i32>::new(leaf);
    let upper_transition = upper.transition_handle();
    let _upper = navigator.push(upper);
    harness.tick();

    upper_transition
        .controller()
        .expect("installed")
        .set_value(0.6);
    assert!(
        (lower_transition.secondary_animation().value() - 0.6).abs() < 1e-6,
        "same TransitionGroup, so the two coordinate"
    );
}

// ============================================================================
// pop — routes.dart:84-94, :177, :308-317
// ============================================================================

/// `finishedWhenPopped => _controller!.isDismissed && !_popFinalized`
/// (`routes.dart:177`): a popped page with a running exit transition is **not**
/// disposed. Its overlay entry survives until the animation reaches `dismissed`,
/// which raises `finalize()`.
#[test]
fn popped_page_route_keeps_its_overlay_entry_until_the_exit_transition_dismisses() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let route = PageRoute::<i32>::new(leaf);
    let transition = route.transition_handle();
    let _result = navigator.push(route);
    harness.tick();
    complete_entrance(&transition, &mut harness);

    let popped = *navigator.route_ids().last().expect("pushed");
    assert_eq!(navigator.tracked_entry_count(), 2);

    assert!(navigator.pop());
    harness.tick();

    assert!(
        navigator.entry_of(popped).is_some(),
        "the route is popped but its exit transition is still running"
    );
    assert!(!transition.is_pop_finalized());

    transition.controller().expect("installed").set_value(0.0);
    harness.tick();

    assert!(transition.is_pop_finalized());
    assert_eq!(
        navigator.tracked_entry_count(),
        1,
        "the finalized route's overlay entry is dropped"
    );
    assert!(navigator.entry_of(popped).is_none());
    assert_eq!(navigator.route_ids().len(), 1);
}

/// `PopupRoute` pops the same way; the barrier is what makes it dismissible, not
/// the pop path.
#[test]
fn popped_popup_route_finalizes_on_dismissal() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let route = PopupRoute::<i32>::new(leaf).barrier_color(Color::RED);
    let transition = route.transition_handle();
    let _result = navigator.push(route);
    harness.tick();
    complete_entrance(&transition, &mut harness);

    assert!(navigator.pop());
    harness.tick();
    assert_eq!(navigator.tracked_entry_count(), 2, "still animating out");

    transition.controller().expect("installed").set_value(0.0);
    harness.tick();

    assert_eq!(navigator.tracked_entry_count(), 1);
    assert_eq!(navigator.route_ids().len(), 1);
}

// ============================================================================
// barrier — routes.dart:2273-2330
// ============================================================================

/// A non-dismissible barrier absorbs pointers and installs no gesture recogniser,
/// so a tap on it cannot pop the route. A dismissible one adds the recogniser.
///
/// **Divergence, not parity:** FLUI has no `ModalBarrier`, no `BlockSemantics`,
/// no `barrierLabel`. The barrier absorbs *pointers* only. That a tap actually
/// pops is proven end-to-end in `tests/routes.rs`, which can dispatch one.
#[test]
fn barrier_absorbs_pointers_and_only_a_dismissible_one_listens_for_the_tap() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();
    let _result = navigator.push(PopupRoute::<i32>::new(leaf));
    harness.tick();

    let names = harness.render_debug_names();
    assert!(
        names.iter().any(|n| n.ends_with("RenderAbsorbPointer")),
        "every modal barrier absorbs pointers: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.ends_with("RenderListener")),
        "a non-dismissible barrier installs no gesture recogniser: {names:?}"
    );

    let (navigator, mut harness, _bottom) = navigator_with_seed();
    let _result = navigator.push(PopupRoute::<i32>::new(leaf).barrier_dismissible(true));
    harness.tick();

    let names = harness.render_debug_names();
    assert!(
        names.iter().any(|n| n.ends_with("RenderListener")),
        "a dismissible barrier listens for the dismiss tap: {names:?}"
    );
}

// ============================================================================
// buildPage / buildTransitions — routes.dart:1229-1240, :1656
// ============================================================================

/// The page builder receives both animations, the transitions builder wraps what
/// it returns, and an animation tick rebuilds both.
#[test]
fn page_and_transitions_builders_receive_both_animations_and_rebuild_on_tick() {
    let (navigator, mut harness, _bottom) = navigator_with_seed();

    let seen = Arc::new(parking_lot::Mutex::new(Vec::<(f32, f32)>::new()));
    let wrapped = Arc::new(AtomicUsize::new(0));

    let route = {
        let seen = Arc::clone(&seen);
        let wrapped = Arc::clone(&wrapped);
        PageRoute::<i32>::new(move |_ctx, animation, secondary| {
            seen.lock().push((animation.value(), secondary.value()));
            SizedBox::new(10.0, 10.0).into_view().boxed()
        })
        .transitions(move |_ctx, _animation, _secondary, child| {
            wrapped.fetch_add(1, Ordering::Relaxed);
            child
        })
    };
    let transition = route.transition_handle();
    let _result = navigator.push(route);
    harness.tick();

    assert_eq!(
        seen.lock().first().copied(),
        Some((0.0, 0.0)),
        "the first build sees a dismissed entrance and no route above"
    );
    assert!(wrapped.load(Ordering::Relaxed) >= 1, "transitions ran");

    // The relay fires on the tick, the `AnimatedView` marks the scope dirty, and
    // both builders re-run with the new value.
    let builds = seen.lock().len();
    transition.controller().expect("installed").set_value(0.5);
    harness.tick();

    let samples = seen.lock().clone();
    assert!(
        samples.len() > builds,
        "an animation tick must rebuild the modal scope"
    );
    assert!(
        (samples.last().expect("a build").0 - 0.5).abs() < 1e-6,
        "the page builder sees the current animation value, got {samples:?}"
    );
}
