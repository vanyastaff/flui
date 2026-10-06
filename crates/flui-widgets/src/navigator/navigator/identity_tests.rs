//! Identity-exhaustion cases that need the navigator's private admission seams.

use super::*;

pub(in crate::navigator) fn navigator_command_identity_exhaustion_preserves_target_authority() {
    use crate::navigator::overlay_route::SimpleRoute;
    let counter = AtomicU64::new(u64::MAX - 2);
    let original = NavigatorHandle::new();
    original.push(SimpleRoute::<()>::new(|_| {
        crate::Text::new("original").boxed()
    }));
    let original_route = original.current().expect("original route");
    let original_target = register_command_target_with_id(
        &original.shared,
        NavigatorCommandTargetId::next_from(&counter),
    );
    let replacement = NavigatorHandle::new();
    replacement.push(SimpleRoute::<()>::new(|_| {
        crate::Text::new("replacement").boxed()
    }));
    let replacement_route = replacement.current().expect("replacement route");
    let replacement_target = register_command_target_with_id(
        &replacement.shared,
        NavigatorCommandTargetId::next_from(&counter),
    );
    assert_ne!(original_target.id, replacement_target.id);
    for _ in 0..3 {
        let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            register_command_target_with_id(
                &replacement.shared,
                NavigatorCommandTargetId::next_from(&counter),
            )
        }))
        .expect_err("exhausted command identity cannot replace authority");
        assert_eq!(
            flui_foundation::panic::payload_text(failure.as_ref()),
            Some("BUG: navigator command target identity capacity exhausted")
        );
    }
    assert_eq!(
        NavigatorCommand::remove_route(original_target, original_route).apply_on_owner(),
        Ok(NavigatorCommandOutcome::Removed(true))
    );
    assert_eq!(original.current(), None);
    assert_eq!(replacement.current(), Some(replacement_route));
    drop(original);
    assert_eq!(
        NavigatorCommand::pop(original_target).apply_on_owner(),
        Err(NavigatorCommandError::OwnerGone)
    );
    assert_eq!(
        NavigatorCommand::remove_route(replacement_target, replacement_route).apply_on_owner(),
        Ok(NavigatorCommandOutcome::Removed(true))
    );
    assert_eq!(replacement.current(), None);
    let independent = NavigatorHandle::new();
    independent.push(SimpleRoute::<()>::new(|_| {
        crate::Text::new("independent").boxed()
    }));
    let route = independent.current().expect("independent route");
    assert_eq!(
        NavigatorCommand::remove_route(independent.command_target(), route).apply_on_owner(),
        Ok(NavigatorCommandOutcome::Removed(true))
    );
    assert_eq!(independent.current(), None);
}

use crate::support::child_process::child_test;

// Each case runs in its own child process: the failure it guards against is an
// abort from a competing destructor during capacity unwind (ADR-0127), and the
// named-operation cases spend the process-global identity counters.

child_test! {
    fn admission_at_the_last_identity_is_healthy() {
        assert_identity_admission_custody("healthy");
    }
}
child_test! {
    fn admission_refusal_retains_the_route() {
        assert_identity_admission_custody("route");
    }
}
child_test! {
    fn admission_refusal_retains_the_commit() {
        assert_identity_admission_custody("commit");
    }
}
child_test! {
    fn admission_refusal_retains_route_and_commit() {
        assert_identity_admission_custody("both");
    }
}
child_test! {
    fn batch_refusal_retains_the_routes() {
        assert_identity_batch_admission_custody("tail-route");
    }
}
child_test! {
    fn batch_refusal_retains_the_commit() {
        assert_identity_batch_admission_custody("tail-commit");
    }
}
child_test! {
    fn batch_refusal_retains_routes_and_commit() {
        assert_identity_batch_admission_custody("tail-both");
    }
}
child_test! {
    fn batch_overlay_refusal_publishes_nothing() {
        assert_identity_batch_admission_custody("tail-entry");
    }
}
child_test! {
    fn pop_and_push_named_refusal_keeps_the_departing_route() {
        assert_named_refusal_changes_nothing("pop_and_push_named");
    }
}
child_test! {
    fn pop_and_push_named_with_refusal_retains_result_and_request() {
        assert_named_refusal_changes_nothing("pop_and_push_named_with");
    }
}
child_test! {
    fn push_named_typed_refusal_retains_the_request() {
        assert_named_refusal_changes_nothing("push_named_typed");
    }
}

/// A user value whose destructor panics once armed: dropping it during a
/// capacity unwind would abort the process.
struct ArmedPayload {
    armed: Arc<AtomicBool>,
    drops: Arc<AtomicU32>,
}

impl Drop for ArmedPayload {
    fn drop(&mut self) {
        if self.armed.load(Ordering::SeqCst) {
            self.drops.fetch_add(1, Ordering::SeqCst);
            panic!("competing payload destruction");
        }
    }
}

/// A named operation refused at route-identity exhaustion fails whole: the
/// departing route stays, nothing is pushed, the caller's request arguments
/// and result are retained rather than dropped during unwind, and the same
/// handle keeps navigating.
fn assert_named_refusal_changes_nothing(operation: &str) {
    use crate::navigator::named_route::RouteRequest;
    use crate::navigator::overlay_route::SimpleRoute;
    let armed = Arc::new(AtomicBool::new(false));
    let drops = Arc::new(AtomicU32::new(0));
    let payload = || ArmedPayload {
        armed: Arc::clone(&armed),
        drops: Arc::clone(&drops),
    };
    let navigator = NavigatorHandle::new();
    navigator.route("/next", |_request: &RouteRequest<'_>| {
        Some(SimpleRoute::<()>::new(|_| crate::Text::new("next").boxed()))
    });
    navigator.push(SimpleRoute::<()>::new(|_| crate::Text::new("base").boxed()));
    navigator.push(SimpleRoute::<()>::new(|_| {
        crate::Text::new("departing").boxed()
    }));
    let before = navigator.route_ids();
    let departing = navigator.current().expect("departing route");
    RouteId::leave_process_identities(0);
    let request = RouteSettings::named("/next").with_arguments(payload());
    let result = (operation == "pop_and_push_named_with").then(payload);
    armed.store(true, Ordering::SeqCst);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match operation {
        "pop_and_push_named" => navigator.pop_and_push_named(request).map(|_| ()),
        "pop_and_push_named_with" => navigator
            .pop_and_push_named_with(request, result.expect("caller result"))
            .map(|_| ()),
        "push_named_typed" => navigator.push_named_typed::<()>(request).map(|_| ()),
        other => unreachable!("unknown named operation {other}"),
    }))
    .expect_err("route identity refusal fails the operation");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("BUG: route identity capacity exhausted"),
        "the capacity failure stays authoritative"
    );
    assert_eq!(
        drops.load(Ordering::SeqCst),
        0,
        "no caller value dropped during unwind"
    );
    armed.store(false, Ordering::SeqCst);
    assert_eq!(navigator.route_ids(), before, "nothing dismissed or pushed");
    assert_eq!(navigator.current(), Some(departing));
    assert!(navigator.pop(), "the same handle keeps navigating");
    assert_eq!(navigator.route_ids(), before[..1].to_vec());
}

fn assert_identity_admission_custody(case: &str) {
    if case.starts_with("tail-") {
        assert_identity_batch_admission_custody(case);
        return;
    }
    use crate::navigator::overlay_route::{RouteContentBuilder, SimpleRoute};
    use std::cell::Cell;
    struct Capture {
        calls: Rc<Cell<usize>>,
        panics: bool,
        label: &'static str,
    }
    impl Drop for Capture {
        fn drop(&mut self) {
            self.calls.set(self.calls.get() + 1);
            assert!(!self.panics, "{}", self.label);
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
            panics: matches!(case, "route" | "both"),
            label: "competing rejected route destruction",
        },
    };
    let commit_capture = Capture {
        calls: Rc::clone(&commit_calls),
        panics: matches!(case, "commit" | "both"),
        label: "competing rejected commit destruction",
    };
    let counter = AtomicU64::new(if case == "healthy" {
        u64::MAX - 1
    } else {
        u64::MAX
    });
    let navigator = NavigatorHandle::new();
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
            0,
            "incoming unwind retains rejected route ownership"
        );
        assert_eq!(
            commit_calls.get(),
            0,
            "incoming unwind retains independent commit ownership"
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
        panics: bool,
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
            assert!(!self.panics, "competing rejected batch route destruction");
        }
    }
    struct CommitCapture {
        calls: Rc<Cell<usize>>,
        panics: bool,
    }
    impl Drop for CommitCapture {
        fn drop(&mut self) {
            self.calls.set(self.calls.get() + 1);
            assert!(!self.panics, "competing rejected batch commit destruction");
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
        panics: matches!(case, "tail-route" | "tail-both"),
    };
    let commit = CommitCapture {
        calls: Rc::clone(&commit_calls),
        panics: matches!(case, "tail-commit" | "tail-both"),
    };
    let navigator = NavigatorHandle::new();
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
        0,
        "prepared, remaining and top rejected routes retain independent custody"
    );
    assert_eq!(commit_calls.get(), 0);
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
