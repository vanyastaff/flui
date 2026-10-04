//! Runner for table-driven tests: each row is a named scenario, and a failing
//! row is reported under its name.

use std::panic::{AssertUnwindSafe, catch_unwind};

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
            crate::panic::retain_opaque_payload(payload);
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

#[cfg(test)]
mod tests {
    use super::run_cases;

    struct Bomb;

    impl Drop for Bomb {
        fn drop(&mut self) {
            panic!("payload destructor");
        }
    }

    fn payload_with_a_panicking_destructor() {
        std::panic::panic_any(Bomb);
    }

    fn aggregate_payload_with_panicking_destructors() {
        std::panic::panic_any((Bomb, Bomb));
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
            (
                "aggregate",
                aggregate_payload_with_panicking_destructors as fn(),
            ),
            ("later", later_failure as fn()),
        ]);
    }
}
