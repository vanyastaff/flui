//! Runner for table-driven tests: each row is a named scenario, and a failing
//! row is reported under its name.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs every `(name, scenario)` row; the first failure names its row.
pub(crate) fn run_cases(cases: &[(&str, fn())]) {
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
            panic!("case `{name}` failed: {message}");
        }
    }
}
