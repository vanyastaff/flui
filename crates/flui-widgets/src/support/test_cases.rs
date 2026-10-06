//! Runner for in-crate table-driven tests: each row is a named scenario, and
//! a failing row is reported under its name.

use std::panic::{AssertUnwindSafe, catch_unwind};

type Case = (&'static str, fn());

/// Runs every row, then panics once naming each row that failed.
pub(crate) fn run_cases(family: &str, cases: &[Case]) {
    let mut failures = Vec::new();
    for (name, case) in cases {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(case)) {
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("<non-string panic payload>");
            failures.push(format!("  {name}: {message}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{family}: {} of {} rows failed:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
}
