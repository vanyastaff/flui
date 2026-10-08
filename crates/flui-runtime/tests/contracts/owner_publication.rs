use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use flui_runtime::owner::{OwnerHost, PublicationError};
use flui_runtime::presentation::PresentationWindow;
use flui_runtime::ui_runtime::{RuntimeHostServices, UiRuntime};

static_assertions::assert_not_impl_any!(OwnerHost: Send, Sync);

pub(super) fn window() -> PresentationWindow {
    let window = flui_platform::headless_platform()
        .open_window(flui_platform::WindowOptions::default())
        .expect("headless window");
    let accessibility = window.accessibility();
    PresentationWindow::new(window, accessibility)
}

pub(super) fn runtime() -> UiRuntime {
    runtime_with_wake(Arc::new(|| {}))
}

pub(super) fn runtime_with_wake(wake: Arc<dyn Fn() + Send + Sync>) -> UiRuntime {
    UiRuntime::new(
        window(),
        1.0,
        RuntimeHostServices::new(
            wake,
            Arc::new(AtomicBool::new(false)),
            Arc::new(flui_platform_api::InMemoryClipboard::new()),
            &flui_painting::FontCollection::new(),
            flui_scheduler::ClockSource::Platform,
        ),
    )
    .expect("runtime")
}

struct RetirementCapture {
    retired: Arc<AtomicUsize>,
    failure: Option<&'static str>,
}

impl Drop for RetirementCapture {
    fn drop(&mut self) {
        self.retired.fetch_add(1, Ordering::SeqCst);
        if let Some(failure) = self.failure {
            std::panic::panic_any(failure);
        }
    }
}

fn cancelled_proposal_preserves_retirement_and_retention_policy() {
    for outer_failure in [false, true] {
        for (first, second) in [
            (None, None),
            (Some("runtime capture"), None),
            (None, Some("factory capture")),
            (Some("runtime capture"), Some("factory capture")),
        ] {
            let owner = OwnerHost::new();
            let wake_retired = Arc::new(AtomicUsize::new(0));
            let local_retired = Arc::new(AtomicUsize::new(0));
            let wake_capture = RetirementCapture {
                retired: Arc::clone(&wake_retired),
                failure: second,
            };
            let runtime = runtime_with_wake(Arc::new(move || {
                std::hint::black_box(&wake_capture);
            }));
            let local_capture = RetirementCapture {
                retired: Arc::clone(&local_retired),
                failure: first,
            };
            let post_frame = runtime.owner_frame().local_post_frame_handle();
            post_frame
                .schedule_local(move |_| drop(local_capture))
                .expect("live runtime accepts capture");
            let prepared = owner.prepare_runtime(runtime);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let permit = owner.publication(prepared).expect("reserve publication");
                assert!(!outer_failure, "outer failure");
                drop(permit);
            }));
            // ADR-0136 retains post-frame callbacks during an existing unwind,
            // but closes their lane before retaining the opaque captures.
            assert_eq!(
                local_retired.load(Ordering::SeqCst),
                usize::from(!outer_failure)
            );
            // ADR-0127 retains the last factory's opaque captures after a held
            // runtime failure as well as during an existing unwind.
            assert_eq!(
                wake_retired.load(Ordering::SeqCst),
                usize::from(!outer_failure && first.is_none())
            );
            assert!(
                post_frame.schedule_local(|_| {}).is_err(),
                "retention cannot leave callback admission open"
            );
            let expected = if outer_failure {
                Some("outer failure")
            } else {
                first.or(second)
            };
            match expected {
                Some(expected) => {
                    let failure = result.expect_err("first failure propagates");
                    assert_eq!(failure.downcast_ref::<&str>(), Some(&expected));
                }
                None => result.expect("healthy cancellation"),
            }
            let _ = owner
                .publication(owner.prepare_runtime(self::runtime()))
                .expect("publication remains usable after cancellation")
                .commit();
        }
    }
}

fn separately_prepared_siblings_keep_distinct_publication_identities() {
    let owner = OwnerHost::new();
    let primary = owner
        .publication(owner.prepare_runtime(runtime()))
        .expect("primary publication")
        .commit();
    let first = owner
        .prepare_presentation(primary, window())
        .expect("first prepared sibling");
    let second = owner
        .prepare_presentation(primary, window())
        .expect("second prepared sibling");
    assert_eq!(
        first.address().ui_runtime_id,
        second.address().ui_runtime_id
    );
    assert_ne!(first.address(), second.address());
    let first = owner.publication(first).expect("first commit").commit();
    let second = owner.publication(second).expect("second commit").commit();
    for authorizer in [first, second] {
        let next = owner
            .prepare_presentation(authorizer, window())
            .expect("each published sibling independently authorizes shared assembly");
        let _ = owner.publication(next).expect("descendant commit").commit();
    }
    assert_eq!(owner.runtime_count(), 1);
}

fn shared_preparation_requires_the_exact_authorizing_presentation() {
    let owner = OwnerHost::new();
    let primary = owner
        .publication(owner.prepare_runtime(runtime()))
        .expect("primary publication")
        .commit();
    let uninstalled = owner.prepare_runtime(runtime());
    for (authorizer, expected) in [
        (uninstalled.address(), PublicationError::UnknownRuntime),
        (
            flui_foundation::PresentationAddress {
                ui_runtime_id: primary.ui_runtime_id,
                presentation_id: uninstalled.address().presentation_id,
            },
            PublicationError::UnknownPresentation,
        ),
    ] {
        let refused = owner
            .prepare_presentation(authorizer, window())
            .expect_err("uninstalled authorizer cannot expand the forest");
        assert_eq!(refused.error, expected);
        let next = owner
            .prepare_presentation(primary, refused.window)
            .expect("refusal returns the window for a valid authorizer");
        let _ = owner.publication(next).expect("valid publication").commit();
    }
}

fn only_the_originating_host_can_publish_a_prepared_runtime() {
    let owner = OwnerHost::new();
    let other = OwnerHost::new();
    let prepared = owner.prepare_runtime(runtime());
    let address = prepared.address();
    assert_eq!(owner.runtime_count(), 0);
    let refused = other.publication(prepared).expect_err("foreign owner");
    assert_eq!(refused.error, PublicationError::ForeignOwner);
    assert_eq!(other.runtime_count(), 0);
    let published = owner
        .publication(refused.prepared)
        .expect("returned proposal remains publishable")
        .commit();
    assert_eq!(published, address);
    assert_eq!(owner.runtime_count(), 1);
}

fn competing_publication_returns_the_proposal_for_retry() {
    let owner = OwnerHost::new();
    let first = owner.prepare_runtime(runtime());
    let second = owner.prepare_runtime(runtime());
    let permit = owner.publication(first).expect("first publication");
    assert_eq!(owner.phase(), Err(flui_runtime::owner::DispatchError::Busy));
    assert_eq!(
        owner.runtime_ids(),
        Err(flui_runtime::owner::DispatchError::Busy)
    );
    assert_eq!(
        owner.next_wake(),
        Err(flui_runtime::owner::DispatchError::Busy)
    );
    let refused = owner.publication(second).expect_err("borrow is reserved");
    assert_eq!(refused.error, PublicationError::Busy);
    let _ = permit.commit();
    let _ = owner
        .publication(refused.prepared)
        .expect("retry after commit")
        .commit();
    assert_eq!(owner.runtime_count(), 2);
}

fn owner_retirement_preserves_the_first_failure_and_releases_every_runtime() {
    for (first, second) in [
        (None, None),
        (Some("first runtime"), None),
        (None, Some("second runtime")),
        (Some("first runtime"), Some("second runtime")),
    ] {
        let owner = OwnerHost::new();
        let retired = Arc::new(AtomicUsize::new(0));
        for failure in [first, second] {
            let runtime = runtime();
            let capture = RetirementCapture {
                retired: Arc::clone(&retired),
                failure,
            };
            runtime
                .owner_frame()
                .local_post_frame_handle()
                .schedule_local(move |_| drop(capture))
                .expect("live runtime accepts capture");
            let _ = owner
                .publication(owner.prepare_runtime(runtime))
                .expect("install runtime")
                .commit();
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(owner)));
        assert_eq!(
            retired.load(Ordering::SeqCst),
            2,
            "both runtime callback captures retire"
        );
        match first.or(second) {
            Some(expected) => {
                let failure = result.expect_err("retirement failure must propagate");
                assert_eq!(failure.downcast_ref::<&str>(), Some(&expected));
            }
            None => result.expect("healthy retirement"),
        }
        let next = OwnerHost::new();
        let _ = next
            .publication(next.prepare_runtime(runtime()))
            .expect("fresh host remains usable after containment")
            .commit();
    }
}

fn abandoned_publication_releases_the_registry_before_runtime_retirement() {
    struct OnRetirement {
        owner: OwnerHost,
        calls: Rc<Cell<usize>>,
    }
    impl Drop for OnRetirement {
        fn drop(&mut self) {
            assert_eq!(self.owner.runtime_count(), 0);
            self.calls.set(self.calls.get() + 1);
        }
    }
    let owner = OwnerHost::new();
    let runtime = runtime();
    let calls = Rc::new(Cell::new(0));
    let capture = OnRetirement {
        owner: owner.clone(),
        calls: Rc::clone(&calls),
    };
    runtime
        .owner_frame()
        .local_post_frame_handle()
        .schedule_local(move |_| drop(capture))
        .expect("live runtime accepts the callback");
    let prepared = owner.prepare_runtime(runtime);
    let permit = owner.publication(prepared).expect("reserve publication");
    drop(permit);
    assert_eq!(calls.get(), 1, "the unexecuted callback capture retired");
    let _ = owner
        .publication(owner.prepare_runtime(self::runtime()))
        .expect("registry remains usable after retirement reentry")
        .commit();
    assert_eq!(owner.runtime_count(), 1);
}

#[test]
fn owner_publication_contract() {
    crate::table_test::run_table(
        "owner_publication_contract",
        &[
            (
                "cancelled_proposal_preserves_retirement_and_retention_policy",
                cancelled_proposal_preserves_retirement_and_retention_policy as fn(),
            ),
            (
                "owner_retirement_preserves_the_first_failure_and_releases_every_runtime",
                owner_retirement_preserves_the_first_failure_and_releases_every_runtime as fn(),
            ),
            (
                "separately_prepared_siblings_keep_distinct_publication_identities",
                separately_prepared_siblings_keep_distinct_publication_identities as fn(),
            ),
            (
                "shared_preparation_requires_the_exact_authorizing_presentation",
                shared_preparation_requires_the_exact_authorizing_presentation as fn(),
            ),
            (
                "only_the_originating_host_can_publish_a_prepared_runtime",
                only_the_originating_host_can_publish_a_prepared_runtime as fn(),
            ),
            (
                "competing_publication_returns_the_proposal_for_retry",
                competing_publication_returns_the_proposal_for_retry as fn(),
            ),
            (
                "abandoned_publication_releases_the_registry_before_runtime_retirement",
                abandoned_publication_releases_the_registry_before_runtime_retirement as fn(),
            ),
        ],
    );
}
