//! The `HeroController` measurement skeleton.
//!
//! These tests prove that the seams built for measurement — `box_size` / `transform_to`,
//! post-frame after layout, observer attachment, `RouteSubtree`,
//! `LocalPostFrameHandle`, notification outside the history lock, and the
//! offstage animation proxies — **compose** into a destination rect.
//!
//! They do not prove the flight overlay itself; `hero_flight.rs` owns that layer.
//! The private flight-validity predicate keeps a unit test in
//! `src/navigator/hero_controller_tests.rs`.

use std::sync::Arc;
use std::time::Duration;

use flui_view::ViewExt;
use flui_view::prelude::*;

use flui_widgets::__test_access::HeroControllerProbe as _;
use flui_widgets::SizedBox;
use flui_widgets::navigator::{
    HeroController, Navigator, NavigatorHandle, NavigatorObserver, PageRoute, SimpleRoute,
};

use crate::common::harness::{Harness, mount};

const TRANSITION: Duration = Duration::from_millis(300);

fn seeded_navigator() -> NavigatorHandle {
    let navigator = NavigatorHandle::new();
    navigator.seed_initial(SimpleRoute::<i32>::new(|_ctx| {
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    navigator
}

fn page_route() -> PageRoute<i32> {
    PageRoute::<i32>::new(|_ctx, _primary, _secondary| {
        SizedBox::new(30.0, 18.0).into_view().boxed()
    })
    .transition_duration(TRANSITION)
}

/// A root that can drop its `Navigator`, so a controller can be left holding a
/// detached handle.
#[derive(Clone)]
struct Root {
    navigator: NavigatorHandle,
    show: bool,
}

impl View for Root {
    fn create_element(&self) -> flui_view::element::ElementKind {
        flui_view::element::ElementKind::stateless(self)
    }
}

impl StatelessView for Root {
    fn build(&self, _ctx: &dyn BuildContext) -> impl IntoView {
        if self.show {
            Navigator::new(self.navigator.clone()).boxed()
        } else {
            flui_widgets::Text::new("gone").boxed()
        }
    }
}

fn mount_navigator(navigator: &NavigatorHandle) -> Harness {
    mount(Root {
        navigator: navigator.clone(),
        show: true,
    })
}

fn install(navigator: &NavigatorHandle) -> Arc<HeroController> {
    let controller = HeroController::new();
    navigator.add_observer(Arc::clone(&controller) as Arc<dyn NavigatorObserver>);
    controller
}

// ============================================================================
// Attachment
// ============================================================================

// ============================================================================
// Scheduling
// ============================================================================

// ============================================================================
// Measurement — the whole point
// ============================================================================

// ============================================================================
// Staleness and safety
// ============================================================================

/// The measurement is scheduled from an observer callback, which runs with no
/// navigator lock held. Installing a `HeroController` — which reaches back
/// through its owner-local `NavigatorHandle` for `route_modal`, `route_peer` and
/// `post_frame_handle` from inside `did_change_top` — must not deadlock.
///
/// Red-check: in `NavigatorShared::mutate`, call `apply(outcome)` inside the
/// `history.lock()` scope; this test hangs.
#[test]
fn a_hero_controller_does_not_deadlock_the_observer_callback() {
    let navigator = seeded_navigator();
    let controller = install(&navigator);

    let mut harness = mount_navigator(&navigator);
    let _first = harness.enter_owner_scope(|| navigator.push(page_route()));
    harness.tick();
    let _second = harness.enter_owner_scope(|| navigator.push(page_route()));
    harness.tick();
    assert!(harness.enter_owner_scope(|| navigator.pop()));
    harness.tick();

    assert_eq!(controller.measurements().len(), 2);
}

// ============================================================================
// Hero discovery and manifests
// ============================================================================
