use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use parking_lot::Mutex;

use super::{Called, Field, call, install, retire};

type Handler = Box<dyn FnMut() -> u32 + Send>;

/// Handlers kept as one field of a larger state, as hosts keep them.
#[derive(Default)]
struct State {
    handler: Option<Handler>,
    other: u32,
}

fn slot(state: &Mutex<State>) -> Field<'_, Mutex<State>, Handler> {
    Field::new(state, |state| &mut state.handler)
}

/// Records its drop, and whether the state lock was free when it happened.
struct Witness {
    state: Arc<Mutex<State>>,
    dropped: Arc<AtomicBool>,
    dropped_under_lock: Arc<AtomicBool>,
    panics: bool,
}

impl Drop for Witness {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
        if self.state.try_lock().is_none() {
            self.dropped_under_lock.store(true, Ordering::SeqCst);
        }
        assert!(!self.panics, "captured value failed in Drop");
    }
}

struct Observed {
    dropped: Arc<AtomicBool>,
    dropped_under_lock: Arc<AtomicBool>,
}

fn witness(state: &Arc<Mutex<State>>, panics: bool) -> (Witness, Observed) {
    let dropped = Arc::new(AtomicBool::new(false));
    let dropped_under_lock = Arc::new(AtomicBool::new(false));
    (
        Witness {
            state: Arc::clone(state),
            dropped: Arc::clone(&dropped),
            dropped_under_lock: Arc::clone(&dropped_under_lock),
            panics,
        },
        Observed {
            dropped,
            dropped_under_lock,
        },
    )
}

#[test]
fn a_vacant_slot_runs_nothing() {
    let state = Mutex::new(State::default());
    let called = call(&slot(&state), |handler| handler());
    assert!(matches!(called, Called::Vacant));
    assert_eq!(call(&slot(&state), |handler| handler()).or_resume(9), 9);
}

/// The handler runs with the slot's lock released, so it reaches the same
/// state, and goes back to its slot afterwards.
#[test]
fn a_handler_runs_outside_the_lock_and_returns_to_its_slot() {
    let state = Arc::new(Mutex::new(State::default()));
    let inner = Arc::clone(&state);
    state.lock().handler = Some(Box::new(move || {
        let mut state = inner
            .try_lock()
            .expect("the slot's lock is held while its handler runs");
        state.other += 1;
        state.other
    }));
    assert_eq!(call(&slot(&state), |handler| handler()).or_resume(0), 1);
    assert_eq!(call(&slot(&state), |handler| handler()).or_resume(0), 2);
}

/// A handler installed while one runs stays; the one that ran is dropped
/// after the call, outside the lock.
#[test]
fn a_handler_installed_during_a_call_replaces_the_one_that_ran() {
    let state = Arc::new(Mutex::new(State::default()));
    let (capture, observed) = witness(&state, false);
    let inner = Arc::clone(&state);
    state.lock().handler = Some(Box::new(move || {
        let _ = &capture;
        install(&slot(&inner), Box::new(|| 2));
        1
    }));
    assert_eq!(call(&slot(&state), |handler| handler()).or_resume(0), 1);
    assert!(observed.dropped.load(Ordering::SeqCst));
    assert!(!observed.dropped_under_lock.load(Ordering::SeqCst));
    assert_eq!(call(&slot(&state), |handler| handler()).or_resume(0), 2);
}

/// Installing drops the displaced handler outside the lock, and contains its
/// destructor's panic.
#[test]
fn installing_retires_the_displaced_handler_outside_the_lock() {
    let state = Arc::new(Mutex::new(State::default()));
    let (capture, observed) = witness(&state, true);
    state.lock().handler = Some(Box::new(move || {
        let _ = &capture;
        1
    }));
    install(&slot(&state), Box::new(|| 2));
    assert!(observed.dropped.load(Ordering::SeqCst));
    assert!(!observed.dropped_under_lock.load(Ordering::SeqCst));
    assert_eq!(call(&slot(&state), |handler| handler()).or_resume(0), 2);
}

/// A handler's panic comes back as the call's failure with its own payload,
/// and the handler keeps its place.
#[test]
fn a_panicking_handler_keeps_its_place_and_its_payload() {
    let state = Mutex::new(State::default());
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&calls);
    state.lock().handler = Some(Box::new(move || {
        let call = counted.fetch_add(1, Ordering::SeqCst);
        assert_ne!(call, 0, "first call fails");
        7
    }));
    let Called::Panicked(failure) = call(&slot(&state), |handler| handler()) else {
        panic!("the handler's panic is the call's failure");
    };
    let resumed = catch_unwind(AssertUnwindSafe(|| failure.resume())).expect_err("resumed");
    assert!(
        flui_foundation::panic::payload_text(&*resumed)
            .is_some_and(|text| text.contains("first call fails"))
    );
    assert_eq!(call(&slot(&state), |handler| handler()).or_resume(0), 7);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

/// After a handler panics, the handler it was displaced by stays and the
/// one that ran is retained: its destructor, which would fail a second time,
/// never runs, and the first failure is the one reported.
#[test]
fn a_failing_call_retains_the_handler_it_displaced() {
    let state = Arc::new(Mutex::new(State::default()));
    let (capture, observed) = witness(&state, true);
    let inner = Arc::clone(&state);
    state.lock().handler = Some(Box::new(move || {
        let _ = &capture;
        install(&slot(&inner), Box::new(|| 2));
        panic!("first failure");
    }));
    let called = call(&slot(&state), |handler| handler());
    let failure = catch_unwind(AssertUnwindSafe(|| called.or_resume(0))).expect_err("resumed");
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("first failure")
    );
    assert!(!observed.dropped.load(Ordering::SeqCst), "retained");
    assert_eq!(call(&slot(&state), |handler| handler()).or_resume(0), 2);
}

/// A contained failure answers the fallback.
#[test]
fn a_contained_failure_answers_the_fallback() {
    let state = Mutex::new(State::default());
    state.lock().handler = Some(Box::new(|| panic!("contained")));
    assert_eq!(
        call(&slot(&state), |handler| handler()).or_contain(5, "test handler"),
        5
    );
}

#[test]
fn a_closed_slot_does_not_take_its_handler_back() {
    let state = Mutex::new(State::default());
    let open = AtomicBool::new(true);
    state.lock().handler = Some(Box::new(|| 1));
    let closing = slot(&state).restoring_while(|| open.load(Ordering::SeqCst));
    assert_eq!(
        call(&closing, |handler| {
            open.store(false, Ordering::SeqCst);
            handler()
        })
        .or_resume(0),
        1
    );
    assert!(state.lock().handler.is_none());
}

/// While the thread is already unwinding, a retired value is retained.
#[test]
fn retiring_during_an_unwind_retains_the_value() {
    let state = Arc::new(Mutex::new(State::default()));
    let (capture, observed) = witness(&state, true);
    struct RetireOnDrop(Option<Witness>);
    impl Drop for RetireOnDrop {
        fn drop(&mut self) {
            retire(self.0.take());
        }
    }
    let failure = catch_unwind(AssertUnwindSafe(|| {
        let _retiring = RetireOnDrop(Some(capture));
        panic!("unrelated failure");
    }))
    .expect_err("the failure propagates");
    assert_eq!(
        flui_foundation::panic::payload_text(&*failure),
        Some("unrelated failure")
    );
    assert!(!observed.dropped.load(Ordering::SeqCst));
}
