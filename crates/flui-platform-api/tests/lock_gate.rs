//! A presentation gate governs every text-store grant, including queued work,
//! and carries owner failures caught while a grant settles.

use std::cell::RefCell;
use std::rc::Rc;

use flui_platform_api::text_store::{
    CommitGate, InMemoryTextStore, LockArbiter, LockGrant, LockOutcome, LockTiming, TextStore,
    TextStoreError,
};

type Log = Rc<RefCell<Vec<&'static str>>>;

fn recording(log: &Log, name: &'static str) -> LockGrant {
    let log = Rc::clone(log);
    LockGrant::read(move |session| {
        assert_eq!(session.document_len().get(), 4);
        log.borrow_mut().push(name);
    })
}

fn queued_pair() -> (Rc<InMemoryTextStore>, CommitGate, Log) {
    let store = InMemoryTextStore::new("text");
    let gate = CommitGate::new();
    store.set_commit_gate(gate.clone());
    gate.set_open(false);
    let log = Rc::new(RefCell::new(Vec::new()));
    let first_log = Rc::clone(&log);
    let first_gate = gate.clone();
    assert_eq!(
        store.request_lock(
            LockGrant::read(move |_| {
                first_log.borrow_mut().push("first");
                first_gate.set_open(false);
            }),
            LockTiming::Async,
        ),
        Ok(LockOutcome::Deferred)
    );
    assert_eq!(
        store.request_lock(recording(&log, "second"), LockTiming::Async),
        Ok(LockOutcome::Deferred)
    );
    gate.set_open(true);
    (store, gate, log)
}

fn deferred_draining_stops_when_a_grant_closes_the_gate() {
    let (store, gate, log) = queued_pair();
    assert_eq!(store.run_deferred_grants(), 1);
    assert_eq!(*log.borrow(), ["first"]);
    assert_eq!(store.run_deferred_grants(), 0);
    gate.set_open(true);
    assert_eq!(store.run_deferred_grants(), 1);
    assert_eq!(*log.borrow(), ["first", "second"]);
    assert_eq!(store.run_deferred_grants(), 0);
}

fn a_sync_request_is_refused_after_an_older_grant_closes_the_gate() {
    let (store, gate, log) = queued_pair();
    assert_eq!(
        store.request_lock(recording(&log, "current"), LockTiming::Sync),
        Err(TextStoreError::SyncLockUnavailable)
    );
    assert_eq!(*log.borrow(), ["first"]);
    gate.set_open(true);
    assert_eq!(store.run_deferred_grants(), 1);
    assert_eq!(*log.borrow(), ["first", "second"]);
    assert_eq!(
        store.request_lock(recording(&log, "next"), LockTiming::Sync),
        Ok(LockOutcome::Granted)
    );
    assert_eq!(*log.borrow(), ["first", "second", "next"]);
}

fn an_async_request_queues_behind_work_left_by_a_closed_gate() {
    let (store, gate, log) = queued_pair();
    assert_eq!(
        store.request_lock(recording(&log, "current"), LockTiming::Async),
        Ok(LockOutcome::Deferred)
    );
    assert_eq!(*log.borrow(), ["first"]);
    gate.set_open(true);
    assert_eq!(store.run_deferred_grants(), 2);
    assert_eq!(*log.borrow(), ["first", "second", "current"]);
    assert_eq!(store.run_deferred_grants(), 0);
}

fn work_queued_inside_a_grant_waits_if_that_grant_closes_the_gate() {
    let store = InMemoryTextStore::new("text");
    let gate = CommitGate::new();
    store.set_commit_gate(gate.clone());
    let log = Rc::new(RefCell::new(Vec::new()));
    let nested_store = Rc::clone(&store);
    let nested_log = Rc::clone(&log);
    let nested_gate = gate.clone();
    assert_eq!(
        store.request_lock(
            LockGrant::read(move |_| {
                nested_log.borrow_mut().push("first");
                assert_eq!(
                    nested_store.request_lock(recording(&nested_log, "second"), LockTiming::Async),
                    Ok(LockOutcome::Deferred)
                );
                nested_gate.set_open(false);
            }),
            LockTiming::Sync,
        ),
        Ok(LockOutcome::Granted)
    );
    assert_eq!(*log.borrow(), ["first"]);
    gate.set_open(true);
    assert_eq!(store.run_deferred_grants(), 1);
    assert_eq!(*log.borrow(), ["first", "second"]);
}

/// An arbiter behind a gate the test holds, with `grants` queued while it
/// was shut, then reopened.
fn queued_behind_a_gate(grants: usize) -> (LockArbiter, CommitGate) {
    let arbiter = LockArbiter::new();
    let gate = CommitGate::new();
    arbiter.set_gate(gate.clone());
    gate.set_open(false);
    for _ in 0..grants {
        assert_eq!(
            arbiter.request(
                LockGrant::read(|_| {}),
                LockTiming::Async,
                &mut |_| {},
                &mut || {}
            ),
            Ok(LockOutcome::Deferred)
        );
    }
    gate.set_open(true);
    (arbiter, gate)
}

fn settle_runs_after_each_grant_releases_its_lock() {
    let (arbiter, _gate) = queued_behind_a_gate(2);
    let arbiter = Rc::new(arbiter);
    let log = Rc::new(RefCell::new(Vec::new()));
    let (opened, settled, probe) = (Rc::clone(&log), Rc::clone(&log), Rc::clone(&arbiter));
    let ran = arbiter.run_deferred(&mut |_| opened.borrow_mut().push("grant"), &mut || {
        settled.borrow_mut().push(if probe.is_locked() {
            "settle under the lock"
        } else {
            "settle"
        });
    });
    assert_eq!(ran, 2);
    assert_eq!(*log.borrow(), ["grant", "settle", "grant", "settle"]);
    log.borrow_mut().clear();
    assert_eq!(
        arbiter.request(
            LockGrant::read(|_| {}),
            LockTiming::Sync,
            &mut |_| log.borrow_mut().push("grant"),
            &mut || log.borrow_mut().push("settle"),
        ),
        Ok(LockOutcome::Granted)
    );
    assert_eq!(*log.borrow(), ["grant", "settle"]);
}

fn a_panicking_settle_reaches_the_gate_and_the_queue_drains() {
    let (arbiter, gate) = queued_behind_a_gate(3);
    let settles = Rc::new(RefCell::new(0));
    let counted = Rc::clone(&settles);
    let ran = arbiter.run_deferred(&mut |_| {}, &mut || {
        *counted.borrow_mut() += 1;
        let settled = *counted.borrow();
        if settled < 3 {
            std::panic::panic_any(format!("owner failure {settled}"));
        }
    });
    assert_eq!(ran, 3, "every queued grant ran");
    assert_eq!(*settles.borrow(), 3, "every grant was settled");
    assert!(!arbiter.is_locked());
    let first = gate
        .take_failure()
        .expect("the first failure waits at the gate");
    assert_eq!(
        flui_foundation::panic::payload_text(&*first),
        Some("owner failure 1"),
        "the first failure stays authoritative"
    );
    flui_foundation::panic::retain_opaque_payload(first);
    assert!(gate.take_failure().is_none(), "a failure is reported once");
}

fn a_panicking_settle_with_no_owner_gate_resumes_after_release() {
    let arbiter = LockArbiter::new();
    let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        arbiter.request(
            LockGrant::read(|_| {}),
            LockTiming::Sync,
            &mut |_| {},
            &mut || panic!("owner failure with no one to report to"),
        )
    }));
    let payload = unwound.expect_err("the failure is not swallowed");
    assert_eq!(
        flui_foundation::panic::payload_text(&*payload),
        Some("owner failure with no one to report to")
    );
    assert!(!arbiter.is_locked(), "the lock was released first");
}

/// An owner listener that removes itself and then panics: the store's clone
/// is its last owner, and destroying it runs the listener's captures. That
/// destruction stays inside the settle's containment, so the listener's own
/// panic is the failure reported and the capture's is retained after it.
fn a_listener_that_removes_itself_and_panics_keeps_the_first_failure() {
    struct PanicsOnDrop;
    impl Drop for PanicsOnDrop {
        fn drop(&mut self) {
            panic!("listener capture destroyed");
        }
    }
    let store = InMemoryTextStore::new("");
    let gate = CommitGate::new();
    store.set_commit_gate(gate.clone());
    let (weak, capture) = (Rc::downgrade(&store), PanicsOnDrop);
    store.set_owner_listener(Some(Rc::new(move || {
        let _keep_alive = &capture;
        if let Some(store) = weak.upgrade() {
            store.set_owner_listener(None);
        }
        panic!("listener failure");
    })));
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        store.request_lock(
            LockGrant::read_write(|session| {
                session.insert_at_selection("a").expect("in range");
            }),
            LockTiming::Sync,
        )
    }));
    assert_eq!(
        outcome.ok(),
        Some(Ok(LockOutcome::Granted)),
        "the grant stands and its failure waits at the gate"
    );
    assert_eq!(store.text(), "a");
    let first = gate
        .take_failure()
        .expect("the listener's failure waits at the gate");
    assert_eq!(
        flui_foundation::panic::payload_text(&*first),
        Some("listener failure"),
        "the listener's failure stays authoritative over its capture's"
    );
    flui_foundation::panic::retain_opaque_payload(first);
    assert!(gate.take_failure().is_none(), "a failure is reported once");
}

#[test]
fn settling_runs_owner_code_outside_the_lock() {
    let cases: &[(&str, fn())] = &[
        (
            "settle_runs_after_each_grant_releases_its_lock",
            settle_runs_after_each_grant_releases_its_lock,
        ),
        (
            "a_panicking_settle_reaches_the_gate_and_the_queue_drains",
            a_panicking_settle_reaches_the_gate_and_the_queue_drains,
        ),
        (
            "a_panicking_settle_with_no_owner_gate_resumes_after_release",
            a_panicking_settle_with_no_owner_gate_resumes_after_release,
        ),
        (
            "a_listener_that_removes_itself_and_panics_keeps_the_first_failure",
            a_listener_that_removes_itself_and_panics_keeps_the_first_failure,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            failures.push(name);
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
    assert!(failures.is_empty(), "failed settle cases: {failures:?}");
}

#[test]
fn queued_text_store_grants_respect_gate_changes() {
    let cases: &[(&str, fn())] = &[
        (
            "deferred_draining_stops_when_a_grant_closes_the_gate",
            deferred_draining_stops_when_a_grant_closes_the_gate,
        ),
        (
            "a_sync_request_is_refused_after_an_older_grant_closes_the_gate",
            a_sync_request_is_refused_after_an_older_grant_closes_the_gate,
        ),
        (
            "an_async_request_queues_behind_work_left_by_a_closed_gate",
            an_async_request_queues_behind_work_left_by_a_closed_gate,
        ),
        (
            "work_queued_inside_a_grant_waits_if_that_grant_closes_the_gate",
            work_queued_inside_a_grant_waits_if_that_grant_closes_the_gate,
        ),
    ];
    let mut failures = Vec::new();
    for &(name, case) in cases {
        if let Err(payload) = std::panic::catch_unwind(case) {
            failures.push(name);
            flui_foundation::panic::retain_opaque_payload(payload);
        }
    }
    assert!(failures.is_empty(), "failed gate cases: {failures:?}");
}
