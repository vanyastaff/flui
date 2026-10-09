//! Runner for table-driven tests: each row is a named scenario, and a failing
//! row is reported under its name.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Exercise the production retirement seam for private raw-kernel scenarios.
pub(crate) fn dispose_controller(controller: &crate::AnimationController) {
    let mut recovery = crate::animation::Retirement::new();
    controller.dispose(&mut recovery.scope());
    recovery.finish();
}

/// Runs every `(name, scenario)` row, then panics once naming each failing row.
pub(crate) fn run_cases(cases: &[(&str, fn())]) {
    let mut failures = Vec::new();
    for (name, case) in cases {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(case)) {
            let message = payload
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| {
                    payload
                        .downcast_ref::<&str>()
                        .map(|text| (*text).to_owned())
                })
                .unwrap_or_else(|| "non-string panic payload".to_owned());
            failures.push(format!("case `{name}` failed: {message}"));
            drop_contained(payload);
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

/// Drops a panic payload without letting a panicking destructor escape the
/// runner: the failure already recorded stays authoritative and later rows
/// still run. A destructor that keeps panicking is leaked after a few rounds.
fn drop_contained(payload: Box<dyn Any + Send>) {
    let mut payload = Some(payload);
    for _ in 0..8 {
        let Some(current) = payload.take() else {
            return;
        };
        if let Err(next) = catch_unwind(AssertUnwindSafe(move || drop(current))) {
            payload = Some(next);
        }
    }
    if let Some(leaked) = payload {
        std::mem::forget(leaked);
    }
}

#[cfg(test)]
mod tests {
    use super::run_cases;

    struct Bomb;

    impl Drop for Bomb {
        fn drop(&mut self) {
            assert!(std::thread::panicking(), "payload destructor");
        }
    }

    fn payload_with_a_panicking_destructor() {
        std::panic::panic_any(Bomb);
    }

    fn later_failure() {
        panic!("later row");
    }

    /// A row whose panic payload panics again when dropped must not mask the
    /// report or stop the rows after it.
    #[test]
    #[should_panic(expected = "case `later` failed: later row")]
    fn a_panicking_payload_destructor_does_not_stop_later_rows() {
        run_cases(&[
            ("bomb", payload_with_a_panicking_destructor as fn()),
            ("later", later_failure as fn()),
        ]);
    }
}
