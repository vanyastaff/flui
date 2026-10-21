//! `reclaim_each` runs every receiver whatever one does, and `FirstPanic` keeps
//! the first failure for the caller to raise once everything has run.

use std::cell::RefCell;

use flui_foundation::memory::{MemoryPressureLevel, ReclaimMemory, reclaim_each};
use flui_foundation::panic::{FirstPanic, payload_text};

struct Named<'a> {
    name: &'static str,
    log: &'a RefCell<Vec<String>>,
    fails: bool,
}

impl ReclaimMemory for Named<'_> {
    fn reclaim(&self, level: MemoryPressureLevel) {
        self.log
            .borrow_mut()
            .push(format!("{} {level:?}", self.name));
        assert!(!self.fails, "{} fails", self.name);
    }
}

fn every_receiver_runs_in_order_and_the_first_failure_is_kept() {
    let log = RefCell::new(Vec::new());
    let receivers = [
        Named {
            name: "first",
            log: &log,
            fails: true,
        },
        Named {
            name: "second",
            log: &log,
            fails: true,
        },
        Named {
            name: "third",
            log: &log,
            fails: false,
        },
    ];
    let mut failure = FirstPanic::new();
    reclaim_each(&receivers, MemoryPressureLevel::Critical, &mut failure);

    assert_eq!(
        *log.borrow(),
        ["first Critical", "second Critical", "third Critical"],
        "a receiver that panics does not keep the later ones from running"
    );
    let raised = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| failure.resume()))
        .expect_err("the held failure is raised");
    assert_eq!(
        payload_text(&*raised),
        Some("first fails"),
        "the first failure is the one that is kept, not the last"
    );
}

fn no_failure_holds_nothing_and_resumes_nothing() {
    let log = RefCell::new(Vec::new());
    let receivers = [Named {
        name: "only",
        log: &log,
        fails: false,
    }];
    let mut failure = FirstPanic::new();
    reclaim_each(&receivers, MemoryPressureLevel::Moderate, &mut failure);
    failure.resume();
}

fn a_held_failure_resumes_as_the_original_panic() {
    let mut failure = FirstPanic::new();
    assert_eq!(
        failure.contain(|| 7),
        Some(7),
        "a clean run gives its value"
    );
    assert_eq!(failure.contain(|| -> u8 { panic!("one") }), None);
    assert_eq!(failure.contain(|| -> u8 { panic!("two") }), None);
    let raised = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| failure.resume()))
        .expect_err("the held failure is raised");
    assert_eq!(payload_text(&*raised), Some("one"));
}

#[test]
fn reclaim_memory() {
    crate::run_table(&[
        (
            "every_receiver_runs_in_order_and_the_first_failure_is_kept",
            every_receiver_runs_in_order_and_the_first_failure_is_kept,
        ),
        (
            "no_failure_holds_nothing_and_resumes_nothing",
            no_failure_holds_nothing_and_resumes_nothing,
        ),
        (
            "a_held_failure_resumes_as_the_original_panic",
            a_held_failure_resumes_as_the_original_panic,
        ),
    ]);
}
