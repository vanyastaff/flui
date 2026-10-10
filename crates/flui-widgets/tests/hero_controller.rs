//! The `HeroController` measurement skeleton.
//!
//! These tests prove that the seams built for measurement — `box_size` / `transform_to`,
//! post-frame after layout, observer attachment, `RouteSubtree`,
//! `PostFrameHandle`, notification outside the history lock, and the
//! offstage animation proxies — **compose** into a destination rect.
//!
//! They do not prove the flight overlay itself; `hero_flight.rs` owns that layer.

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
pub(crate) fn a_hero_controller_does_not_deadlock_the_observer_callback() {
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

pub(crate) fn replacing_a_hero_navigator_retires_routes_after_attachment() {
    let Some(case) = crate::common::child_process::selected_case() else {
        crate::common::child_process::run_rows(
            "contracts::navigator_failure_containment_and_reentrancy",
            &[
                "hero_attachment_retirement",
                "hero_attachment_replacement",
                "hero_attachment_failure",
                "hero_attachment_alias",
            ],
        );
        return;
    };
    let nested = case == "hero_attachment_replacement";
    let fails = case == "hero_attachment_failure";
    let retains_alias = case == "hero_attachment_alias";

    struct OnDrop(Box<dyn Fn()>);
    impl Drop for OnDrop {
        fn drop(&mut self) {
            (self.0)();
        }
    }

    let controller = HeroController::new();
    let replacement = seeded_navigator();
    let final_attachment = seeded_navigator();
    let observed = std::rc::Rc::new(std::cell::Cell::new(false));
    let callback_observed = std::rc::Rc::clone(&observed);
    let callback_controller = Arc::downgrade(&controller);
    let callback_replacement = replacement.clone();
    let callback_final = final_attachment.clone();
    let capture = OnDrop(Box::new(move || {
        eprintln!("entered retired Hero navigator route capture");
        let controller = callback_controller
            .upgrade()
            .expect("controller remains owned");
        let attached = controller
            .navigator()
            .expect("replacement is already attached");
        assert!(attached.is_same(&callback_replacement));
        callback_observed.set(true);
        if nested {
            controller.did_attach(callback_final.clone());
        }
        assert!(!fails, "outgoing Hero navigator capture failed");
    }));
    let old = NavigatorHandle::new();
    old.seed_initial(SimpleRoute::<()>::new(move |_ctx| {
        let _capture = &capture;
        SizedBox::new(10.0, 10.0).into_view().boxed()
    }));
    let alias = retains_alias.then(|| old.clone());
    controller.did_attach(old);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        controller.did_attach(replacement.clone());
    }));
    if fails {
        let failure = result.expect_err("authored retirement failure propagates");
        assert_eq!(
            failure.downcast_ref::<&str>().copied(),
            Some("outgoing Hero navigator capture failed")
        );
    } else {
        assert!(result.is_ok());
    }
    if retains_alias {
        assert!(
            !observed.get(),
            "an independently owned route remains alive"
        );
    }
    drop(alias);
    assert!(observed.get(), "the outgoing route capture must retire");
    assert!(
        controller
            .navigator()
            .expect("attachment committed")
            .is_same(if nested {
                &final_attachment
            } else {
                &replacement
            })
    );
    controller.did_detach();
    assert!(controller.navigator().is_none());
    controller.did_attach(replacement.clone());
    assert!(
        controller
            .navigator()
            .expect("reattached")
            .is_same(&replacement)
    );
    controller.did_detach();
    crate::common::child_process::pass();
}

// ============================================================================
// Hero discovery and manifests
// ============================================================================
