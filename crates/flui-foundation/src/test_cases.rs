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
            crate::panic::discard_payload(payload);
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

    fn first_failure() {
        panic!("first row");
    }

    fn later_failure() {
        panic!("later row");
    }

    /// A failing row must not stop the rows after it, and each is reported.
    #[test]
    #[should_panic(expected = "case `later` failed: later row")]
    fn a_failing_row_does_not_stop_later_rows() {
        run_cases(&[
            ("first", first_failure as fn()),
            ("later", later_failure as fn()),
        ]);
    }
}
