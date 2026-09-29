//! Runner for table-driven tests: each row is a named scenario, every row runs,
//! and the failure report names each row that failed.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs every `(name, scenario)` row and panics with the names of the rows that
/// failed.
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
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
