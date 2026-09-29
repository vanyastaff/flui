//! Table-test runner: every row runs, and one panic names all failing rows.

use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs each `(case name, body)` row and panics once, listing every row whose
/// body panicked with its message.
#[track_caller]
pub(crate) fn run_rows(rows: &[(&str, fn())]) {
    let mut failed = Vec::new();
    for &(name, body) in rows {
        if let Err(payload) = catch_unwind(AssertUnwindSafe(body)) {
            let message = payload
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| payload.downcast_ref::<&str>().copied())
                .unwrap_or("non-string panic");
            failed.push(format!("{name}: {message}"));
        }
    }
    assert!(
        failed.is_empty(),
        "{} of {} rows failed:\n{}",
        failed.len(),
        rows.len(),
        failed.join("\n")
    );
}
