//! Identity-exhaustion cases that need the navigator's private admission seams.

use super::*;
use std::sync::atomic::AtomicU64;

use crate::navigator::route::{Route, RouteSettings};

use crate::support::child_process::child_test;

// Each case runs in its own child process: the cases spend the
// process-global identity counters, and every refusal is checked against a
// fresh one.

child_test! {
    fn admission_at_the_last_identity_is_healthy() {
        assert_identity_admission_custody("healthy");
    }
}
child_test! {
    fn admission_refusal_destroys_route_and_commit() {
        assert_identity_admission_custody("refused");
    }
}
child_test! {
    fn batch_refusal_destroys_routes_and_commit() {
        assert_identity_batch_admission_custody("tail-route");
    }
}
child_test! {
    fn batch_overlay_refusal_publishes_nothing() {
        assert_identity_batch_admission_custody("tail-entry");
    }
}

fn assert_identity_admission_custody(case: &str) {
    use crate::navigator::overlay_route::{RouteContentBuilder, SimpleRoute};
    use std::cell::Cell;
    struct Capture {
        calls: Rc<Cell<usize>>,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.calls.set(self.calls.get() + 1);
        }
    }
    struct RejectedRoute {
        settings: RouteSettings,
        capture: Capture,
    }
    impl Route for RejectedRoute {
        type Output = ();
        fn settings(&self) -> &RouteSettings {
            &self.settings
        }
    }
    impl NavigatorRoute for RejectedRoute {
        fn content_builder(&self) -> RouteContentBuilder {
            std::hint::black_box(&self.capture);
            Rc::new(|_| crate::Text::new("boundary route").boxed())
        }
    }
    let route_calls = Rc::new(Cell::new(0));
    let commit_calls = Rc::new(Cell::new(0));
    let route = RejectedRoute {
        settings: RouteSettings::default(),
        capture: Capture {
            calls: Rc::clone(&route_calls),
        },
    };
    let commit_capture = Capture {
        calls: Rc::clone(&commit_calls),
    };
    let counter = AtomicU64::new(if case == "healthy" {
        u64::MAX - 1
    } else {
        u64::MAX
    });
    let navigator = NavigatorHandle::new(flui_foundation::BuildPhase::default());
    navigator.push(SimpleRoute::<()>::new(|_| crate::Text::new("base").boxed()));
    let base = navigator.current().expect("base route");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        navigator.push_prepared(
            "push",
            route,
            || {
                RouteReservation::reserve_using(
                    || RouteId::next_from(&counter),
                    OverlayEntryId::next,
                )
            },
            move |history, id, route| {
                let _keep = &commit_capture;
                history.push_with_id(id, route).1
            },
        )
    }));
    if case == "healthy" {
        let (admitted, _) = result.expect("last valid identity admits real route");
        assert_eq!(admitted.get(), u64::MAX - 1);
        assert_eq!(navigator.current(), Some(admitted));
        assert_eq!(
            commit_calls.get(),
            1,
            "healthy commit captures retire normally"
        );
        assert_eq!(route_calls.get(), 0, "history owns the admitted route");
        assert!(navigator.pop());
        assert_eq!(
            route_calls.get(),
            1,
            "healthy route retires through actual pop"
        );
    } else {
        let failure = result.expect_err("capacity failure remains authoritative");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("BUG: route identity capacity exhausted")
        );
        assert_eq!(
            route_calls.get(),
            1,
            "the rejected route is destroyed during the unwind"
        );
        assert_eq!(
            commit_calls.get(),
            1,
            "the rejected commit capture is destroyed during the unwind"
        );
        assert_eq!(navigator.current(), Some(base));
        for _ in 0..2 {
            assert!(std::panic::catch_unwind(|| RouteId::next_from(&counter)).is_err());
        }
    }
    assert_eq!(navigator.current(), Some(base));
    navigator.push(SimpleRoute::<()>::new(|_| {
        crate::Text::new("recovery").boxed()
    }));
    assert_ne!(navigator.current(), Some(base));
    assert!(
        navigator.pop(),
        "same handle continues actual healthy navigation after containment"
    );
    assert_eq!(navigator.current(), Some(base));
}

fn assert_identity_batch_admission_custody(case: &str) {
    use crate::navigator::binding::RouteBindingSlot;
    use crate::navigator::overlay_route::{RouteContentBuilder, SimpleRoute};
    use std::cell::Cell;
    struct TailRoute {
        settings: RouteSettings,
        slot: RouteBindingSlot,
        drops: Rc<Cell<usize>>,
        builds: Rc<Cell<usize>>,
    }
    impl Route for TailRoute {
        type Output = ();
        fn settings(&self) -> &RouteSettings {
            &self.settings
        }
    }
    impl NavigatorRoute for TailRoute {
        fn content_builder(&self) -> RouteContentBuilder {
            self.builds.set(self.builds.get() + 1);
            Rc::new(|_| crate::Text::new("tail route").boxed())
        }
        fn binding_slot(&self) -> Option<&RouteBindingSlot> {
            Some(&self.slot)
        }
    }
    impl Drop for TailRoute {
        fn drop(&mut self) {
            self.drops.set(self.drops.get() + 1);
        }
    }
    struct CommitCapture {
        calls: Rc<Cell<usize>>,
    }
    impl Drop for CommitCapture {
        fn drop(&mut self) {
            self.calls.set(self.calls.get() + 1);
        }
    }
    let drops = Rc::new(Cell::new(0));
    let builds = Rc::new(Cell::new(0));
    let commit_calls = Rc::new(Cell::new(0));
    let slots = [
        RouteBindingSlot::new(),
        RouteBindingSlot::new(),
        RouteBindingSlot::new(),
    ];
    let route = |index: usize| TailRoute {
        settings: RouteSettings::default(),
        slot: slots[index].clone(),
        drops: Rc::clone(&drops),
        builds: Rc::clone(&builds),
    };
    let commit = CommitCapture {
        calls: Rc::clone(&commit_calls),
    };
    let navigator = NavigatorHandle::new(flui_foundation::BuildPhase::default());
    navigator.push(SimpleRoute::<()>::new(|_| {
        crate::Text::new("batch base").boxed()
    }));
    let base = navigator.current().expect("batch base");
    let before_history = navigator.route_ids();
    // Two batch members fit; the third reservation is refused — by the route
    // counter, or, for `tail-entry`, by the overlay-entry counter.
    let entry_refused = case == "tail-entry";
    let counter = AtomicU64::new(if entry_refused { 1 } else { u64::MAX - 2 });
    let entries = AtomicU64::new(if entry_refused { u64::MAX - 1 } else { 1 });
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        navigator.replace_tail_using(
            Some(base),
            vec![route(0), route(1)],
            route(2),
            move |ids| {
                let _keep = &commit;
                ids
            },
            || {
                RouteReservation::reserve_using(
                    || RouteId::next_from(&counter),
                    || OverlayEntryId::from_counter(&entries),
                )
            },
        )
    }))
    .expect_err("the last batch reservation refuses before publication");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some(if entry_refused {
            "overlay entry identity space exhausted: 0"
        } else {
            "BUG: route identity capacity exhausted"
        })
    );
    assert_eq!(
        drops.get(),
        3,
        "prepared, remaining and top rejected routes are each destroyed once"
    );
    assert_eq!(
        commit_calls.get(),
        1,
        "the rejected commit capture is destroyed"
    );
    assert_eq!(
        builds.get(),
        0,
        "capacity refusal precedes all builder callbacks"
    );
    assert!(
        slots.iter().all(|slot| slot.get().is_none()),
        "no rejected route was bound"
    );
    assert_eq!(
        navigator.route_ids(),
        before_history,
        "no rejected history entry was admitted"
    );
    assert_eq!(navigator.current(), Some(base));
    let admitted = navigator.replace_tail(
        Some(base),
        vec![SimpleRoute::<()>::new(|_| {
            crate::Text::new("healthy below").boxed()
        })],
        SimpleRoute::<()>::new(|_| crate::Text::new("healthy top").boxed()),
        |ids| ids,
    );
    assert_eq!(admitted.len(), 2);
    assert_eq!(
        navigator.current(),
        admitted.last().copied(),
        "same handle admits a subsequent healthy replacement"
    );
    for id in admitted.into_iter().rev() {
        assert!(navigator.remove_route(id));
    }
    assert_eq!(navigator.current(), Some(base));
}
