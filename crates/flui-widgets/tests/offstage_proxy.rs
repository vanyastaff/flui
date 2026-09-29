//! `ModalRoute.offstage` swaps the **animation proxies**.
//!
//! # Why this exists at all
//!
//! `HeroController` measures a route's *final* geometry one frame before the flight
//! starts. Forcing the route offstage keeps it laid out but unpainted
//! — and that alone is not enough. A route half-way through its entrance
//! transition lays out half-way through its entrance transition, offstage or not.
//! Flutter's setter therefore does two more things (`routes.dart:1958-1961`):
//!
//! ```dart
//! _animationProxy!.parent = _offstage ? kAlwaysCompleteAnimation : super.animation;
//! _secondaryAnimationProxy!.parent = _offstage ? kAlwaysDismissedAnimation : super.secondaryAnimation;
//! ```
//!
//! so `buildPage` and `buildTransitions` see `1.0`/completed and `0.0`/dismissed.
//! *That* is what makes the offstage frame lay out the destination where it will
//! finally rest. This was deferred at first; without it every hero flight would
//! start from the wrong rect, and no test in the tree would have noticed.
//!
//! These tests read the animations the **builders actually receive**, not the
//! route's private cells — a divergence noted earlier.

use std::sync::Arc;
use std::time::Duration;

use flui_animation::AnimationStatus;
use flui_view::prelude::*;
use flui_view::{BoxedView, ViewExt};
use flui_widgets::__test_access::{PageRouteProbe as _, RouteProbe as _, TransitionHandle};
use flui_widgets::SizedBox;
use flui_widgets::navigator::{Navigator, NavigatorHandle, PageRoute, RouteAnimation, SimpleRoute};
use parking_lot::Mutex;

use crate::common::harness::{Harness, mount};

const TRANSITION: Duration = Duration::from_millis(300);

/// What one build of the page saw of its two animations.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Seen {
    primary_value: f64,
    primary_status: AnimationStatus,
    secondary_value: f64,
    secondary_status: AnimationStatus,
}

#[derive(Default)]
struct Sightings(Mutex<Vec<Seen>>);

impl Sightings {
    fn record(&self, primary: &RouteAnimation, secondary: &RouteAnimation) {
        self.0.lock().push(Seen {
            primary_value: primary.value(),
            primary_status: primary.status(),
            secondary_value: secondary.value(),
            secondary_status: secondary.status(),
        });
    }

    fn last(&self) -> Seen {
        *self.0.lock().last().expect("the page has been built")
    }
}

fn seeded_navigator() -> NavigatorHandle {
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    navigator
}

/// A `PageRoute` whose **page builder** records the animations it is handed.
fn recording_route(sightings: &Arc<Sightings>) -> PageRoute<i32> {
    let sightings = Arc::clone(sightings);
    PageRoute::<i32>::new(move |_ctx, primary, secondary| -> BoxedView {
        sightings.record(primary, secondary);
        SizedBox::new(30.0, 18.0).into_view().boxed()
    })
    .transition_duration(TRANSITION)
}

fn mounted(navigator: &NavigatorHandle) -> Harness {
    mount(Navigator::new(navigator.clone()))
}

/// Park the route's controller mid-entrance.
///
/// The harness mounts no `VsyncScope`, so a route's `AnimationController` falls back
/// to its own wall-clock ticker and `Harness::tick_by` cannot drive it (see
/// `binding.rs::RouteVsync`). Every transition-route test in this crate drives
/// the controller by hand for the same reason.
fn park_mid_transition(transition: &TransitionHandle, value: f64) {
    let controller = transition
        .controller()
        .expect("install() created the controller");
    controller.set_value(value);
    controller
        .forward()
        .expect("a parked controller can resume");
}

/// **The core of Task A.** An offstage route's page builder reads
/// `kAlwaysCompleteAnimation` (`routes.dart:1958`; `animations.dart:39-43` —
/// `status == completed`, `value == 1.0`).
///
/// The route is pushed with a real 300 ms transition and forced offstage *before* a
/// single frame runs, so its controller is still at `0.0` and running forward: only
/// the proxy swap can make the builder see `1.0`.
///
/// Red-check: delete the `self.primary.set_parent(…)` arm from
/// `ModalInner::sync_animation_proxies` — the builder reads the live controller.
#[test]
fn an_offstage_routes_page_reads_a_completed_primary_animation() {
    let navigator = seeded_navigator();
    let mut harness = mounted(&navigator);

    let sightings = Arc::new(Sightings::default());
    let route = recording_route(&sightings);
    let modal = route.modal_handle();
    let _result = navigator.push(route);

    modal.set_offstage(true);
    harness.tick();

    let seen = sightings.last();
    assert_eq!(seen.primary_value, 1.0);
    assert_eq!(seen.primary_status, AnimationStatus::Completed);
}

/// The other half of the same setter: an offstage route's `secondaryAnimation` is
/// `kAlwaysDismissedAnimation` (`routes.dart:1959-1961`; `animations.dart:69-73`).
///
/// An offstage route must not be pushed aside by whatever sits above it either —
/// otherwise its measured rect is offset by the *next* route's entrance.
///
/// Red-check: leave the secondary proxy pointed at the live train, i.e. delete the
/// `self.secondary.set_parent(…)` arm from `sync_animation_proxies`.
#[test]
fn an_offstage_routes_page_reads_a_dismissed_secondary_animation() {
    let navigator = seeded_navigator();
    let mut harness = mounted(&navigator);

    let sightings = Arc::new(Sightings::default());
    let route = recording_route(&sightings);
    let modal = route.modal_handle();
    let _result = navigator.push(route);
    harness.tick();

    // Push a second page on top and park *its* entrance half-way: the first route's
    // secondary animation now tracks the newcomer, so "dismissed" cannot be a
    // leftover default.
    let above = PageRoute::<i32>::new(|_ctx, _a, _s| SizedBox::new(5.0, 5.0).into_view().boxed())
        .transition_duration(TRANSITION);
    let above_transition = above.transition_handle();
    let _above = navigator.push(above);
    park_mid_transition(&above_transition, 0.5);
    harness.tick();

    let live = sightings.last();
    assert!(
        live.secondary_value > 0.0,
        "the covered route's secondary animation tracks the route above it: {live:?}"
    );

    modal.set_offstage(true);
    harness.tick();

    let offstage = sightings.last();
    assert_eq!(offstage.secondary_value, 0.0);
    assert_eq!(offstage.secondary_status, AnimationStatus::Dismissed);
}
