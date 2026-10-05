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

/// Selects the one case a re-executed child runs.
const CHILD_CASE: &str = "FLUI_NAVIGATOR_IDENTITY_EXHAUSTION_CASE";
/// Exit status only a child that completed its case reports. libtest exits 0
/// when its filter matches nothing and 101 when a test fails.
const CHILD_COMPLETED: i32 = 73;

/// Each case runs in a child process: the failure it guards against is an
/// abort from a competing destructor during capacity unwind (ADR-0127).
#[test]
fn navigator_identity_exhaustion_retains_admission_ownership() {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::Instant;
    const TEST: &str = "navigator::navigator::identity_tests::navigator_identity_exhaustion_retains_admission_ownership";
    let cases = [
        "healthy",
        "route",
        "commit",
        "both",
        "tail-route",
        "tail-commit",
        "tail-both",
    ];
    if let Ok(case) = std::env::var(CHILD_CASE) {
        assert!(
            cases.contains(&case.as_str()),
            "known identity exhaustion child"
        );
        assert_identity_admission_custody(&case);
        std::process::exit(CHILD_COMPLETED);
    }
    let mut failures = Vec::new();
    for case in cases {
        let mut child = Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", TEST, "--nocapture", "--test-threads=1"])
            .env(CHILD_CASE, case)
            .env("RUST_BACKTRACE", "0")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("identity custody child");
        let mut stdout = child.stdout.take().expect("child stdout");
        let mut stderr = child.stderr.take().expect("child stderr");
        let out = std::thread::spawn(move || {
            let mut text = String::new();
            stdout.read_to_string(&mut text).expect("child output");
            text
        });
        let err = std::thread::spawn(move || {
            let mut text = String::new();
            stderr.read_to_string(&mut text).expect("child errors");
            text
        });
        let started = Instant::now();
        let mut timed_out = false;
        while child.try_wait().expect("child status").is_none() {
            if started.elapsed() > Duration::from_secs(10) {
                timed_out = true;
                child.kill().expect("kill owned timed-out child");
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = child.wait().expect("reap owned child");
        let stdout = out.join().expect("stdout reader");
        let stderr = err.join().expect("stderr reader");
        if timed_out || status.code() != Some(CHILD_COMPLETED) {
            failures.push(format!(
                "{case}: {status}; timeout={timed_out}
{stdout}
{stderr}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{}",
        failures.join(
            "
"
        )
    );
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
        navigator.push_prepared_using(
            "push",
            route,
            move |history, id, route| {
                let _keep = &commit_capture;
                history.push_with_id(id, route).1
            },
            || RouteId::next_from(&counter),
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
    let counter = AtomicU64::new(u64::MAX - 1);
    let failure = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        navigator.replace_tail_using(
            Some(base),
            vec![route(0), route(1)],
            route(2),
            move |ids| {
                let _keep = &commit;
                ids
            },
            || RouteId::next_from(&counter),
        )
    }))
    .expect_err("second batch reservation refuses before publication");
    assert_eq!(
        flui_foundation::panic::payload_text(failure.as_ref()),
        Some("BUG: route identity capacity exhausted")
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
