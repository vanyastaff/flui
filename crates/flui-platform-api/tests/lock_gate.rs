//! A presentation gate governs every text-store grant, including queued work.

use std::cell::RefCell;
use std::rc::Rc;

use flui_platform_api::text_store::{
    CommitGate, InMemoryTextStore, LockGrant, LockOutcome, LockTiming, TextStore, TextStoreError,
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
